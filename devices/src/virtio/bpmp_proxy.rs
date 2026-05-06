use std::collections::BTreeMap;
use std::fs::File;
use std::io::Error as IoError;
use std::io::Read as IoRead;
use std::io::Write as IoWrite;

use anyhow::bail;
use anyhow::Context;
use base::debug;
use base::error;
use base::info;
use base::AsRawDescriptor;
use base::Event;
use base::EventToken;
use base::RawDescriptor;
use base::WaitContext;
use base::WorkerThread;
use snapshot::AnySnapshot;
use thiserror::Error;
use vm_memory::GuestMemory;

use crate::virtio::DescriptorChain;
use crate::virtio::DeviceType;
use crate::virtio::Interrupt;
use crate::virtio::Queue;
use crate::virtio::VirtioDevice;

const BPMP_HOST_DEVICE: &str = "/dev/bpmp_host";

// Size of a packed request or response
const BPMP_MAX_REQ_SIZE: usize = 152;
const BPMP_MAX_RESP_SIZE: usize = 136;

// A single queue of size 2. The guest kernel driver will enqueue a single
// descriptor chain containing one command buffer and one response buffer at a
// time.
const QUEUE_SIZE: u16 = 2;
const QUEUE_SIZES: &[u16] = &[QUEUE_SIZE];

#[derive(Error, Debug)]
enum Error {
    #[error(
        "bpmp proxy: request is too large: {size} > {} bytes",
        BPMP_MAX_REQ_SIZE
    )]
    RequestTooLong { size: usize },
    #[error(
        "bpmp response buffer is too small: {size} < {} bytes",
        BPMP_MAX_RESP_SIZE
    )]
    ResponseBufferTooSmall { size: usize },
    #[error(
        "bpmp host device generated a response that is unexpectedly long: {size} > {} bytes",
        BPMP_MAX_RESP_SIZE
    )]
    ResponseTooLong { size: usize },
    #[error("bpmp proxy: failed to open host device at {}", BPMP_HOST_DEVICE)]
    NoHostDevice,
    #[error("bpmp proxy: driver has not initialized")]
    UninitializedHostDriver,
    #[error("bpmp proxy: failed to read response")]
    BackendReadError { error: IoError },
}

struct Worker {
    queue: Queue,
    backend: File,
}

impl Worker {
    fn perform_work(&mut self, desc: &mut DescriptorChain) -> anyhow::Result<u32> {
        debug!("got {} descriptors", desc.count);

        let request_len = desc.reader.available_bytes();
        if request_len > BPMP_MAX_REQ_SIZE {
            bail!(Error::RequestTooLong { size: request_len });
        }

        let mut request = vec![0u8; request_len];
        desc.reader.read_exact(&mut request)?;

        debug!("writing {} bytes to backend", request_len);
        debug!("req :\n{:02X?}", request);

        let response_len = desc.writer.available_bytes();
        if response_len < BPMP_MAX_RESP_SIZE {
            bail!(Error::ResponseBufferTooSmall { size: response_len });
        }
        let mut response = vec![0u8; response_len];

        // Start backend R/W transaction

        if let Err(e) = self.backend.write_all(&request) {
            match e.raw_os_error() {
                None => info!("not an OS error, continue"),
                Some(libc::ENOENT) => {
                    error!("driver is in a bad state, end transaction early");
                    bail!(Error::UninitializedHostDriver)
                },
                // TODO EBUSY
                Some(code) => info!("backend write failed with non-fatal error: {}", code)
            }
        }

        let n = match self.backend.read(&mut response) {
            Ok(n) => n,
            // TODO EBUSY
            // most errors in read are fatal
            Err(e) => bail!(Error::BackendReadError { error: e })
        };
        debug!("read {} bytes from backend", n);
        if n > BPMP_MAX_RESP_SIZE {
            bail!(Error::ResponseTooLong { size: n });
        }
        debug!("resp :\n{:02X?}", response);
        desc.writer.write_all(&response[..n])?;

        Ok(desc.writer.bytes_written() as u32)
    }

    fn process_queue(&mut self) {
        let mut needs_interrupt = false;

        while let Some(mut avail_desc) = self.queue.pop() {
            let len = match self.perform_work(&mut avail_desc) {
                Ok(len) => len,
                Err(e) => {
                    error!("{:#}", e);
                    0
                }
            };
            self.queue.add_used_with_bytes_written(avail_desc, len);
            needs_interrupt = true;
        }

        if needs_interrupt {
            self.queue.trigger_interrupt();
        }
    }

    fn run(&mut self, kill_evt: Event) -> anyhow::Result<()> {
        #[derive(EventToken)]
        enum Token {
            QueueAvailable,
            Kill,
        }

        let wait_ctx = WaitContext::build_with(&[
            (self.queue.event(), Token::QueueAvailable),
            (&kill_evt, Token::Kill),
        ])
        .context("WaitContext::build_with")?;

        loop {
            let events = wait_ctx.wait().context("wait_ctx.wait")?;
            for event in events.iter().filter(|e| e.is_readable) {
                match event.token {
                    Token::QueueAvailable => {
                        self.queue.event().wait().context("event.wait")?;
                        self.process_queue();
                    }
                    Token::Kill => return Ok(()),
                }
            }
        }
    }
}

/// Virtio device that proxies guest BPMP IPC to the host `/dev/bpmp-host`.
pub struct BpmpDevice {
    backend: Option<File>,
    worker_thread: Option<WorkerThread<Worker>>,
    features: u64,
}

impl BpmpDevice {
    pub fn new(base_features: u64) -> anyhow::Result<BpmpDevice> {
        let backend = File::options()
            .read(true)
            .write(true)
            .open(BPMP_HOST_DEVICE)
            .context(Error::NoHostDevice)?;
        Ok(BpmpDevice {
            backend: Some(backend),
            worker_thread: None,
            features: base_features,
        })
    }
}

impl VirtioDevice for BpmpDevice {
    fn keep_rds(&self) -> Vec<RawDescriptor> {
        match &self.backend {
            Some(f) => vec![f.as_raw_descriptor()],
            None => Vec::new(),
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Bpmp
    }

    fn queue_max_sizes(&self) -> &[u16] {
        QUEUE_SIZES
    }

    fn features(&self) -> u64 {
        self.features
    }

    fn activate(
        &mut self,
        _mem: GuestMemory,
        _interrupt: Interrupt,
        mut queues: BTreeMap<usize, Queue>,
    ) -> anyhow::Result<()> {
        if queues.len() != 1 {
            bail!("expected 1 queue, got {}", queues.len());
        }

        let queue = queues.pop_first().unwrap().1;
        let backend = self.backend.take().context("missing backend")?;

        self.worker_thread = Some(WorkerThread::start("v_bpmp_proxy", move |kill_evt| {
            let mut worker = Worker { queue, backend };
            if let Err(e) = worker.run(kill_evt) {
                error!("bpmp proxy worker thread failed: {:#}", e);
            }
            worker
        }));

        Ok(())
    }

    // Rest of the methods copied from rng.rs

    fn reset(&mut self) -> anyhow::Result<()> {
        if let Some(worker_thread) = self.worker_thread.take() {
            let _worker = worker_thread.stop();
        }
        Ok(())
    }

    fn virtio_sleep(&mut self) -> anyhow::Result<Option<BTreeMap<usize, Queue>>> {
        if let Some(worker_thread) = self.worker_thread.take() {
            let worker = worker_thread.stop();
            return Ok(Some(BTreeMap::from([(0, worker.queue)])));
        }
        Ok(None)
    }

    fn virtio_wake(
        &mut self,
        queues_state: Option<(GuestMemory, Interrupt, BTreeMap<usize, Queue>)>,
    ) -> anyhow::Result<()> {
        if let Some((mem, interrupt, queues)) = queues_state {
            self.activate(mem, interrupt, queues)?;
        }
        Ok(())
    }

    fn virtio_snapshot(&mut self) -> anyhow::Result<AnySnapshot> {
        // `virtio_sleep` ensures there is no pending state, except for the `Queue`s, which are
        // handled at a higher layer.
        AnySnapshot::to_any(())
    }

    fn virtio_restore(&mut self, data: AnySnapshot) -> anyhow::Result<()> {
        let () = AnySnapshot::from_any(data)?;
        Ok(())
    }
}

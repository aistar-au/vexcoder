// Keep common Tokio runtime helpers behind one local seam.
// Attribute macros such as `#[tokio::test]` still remain direct in tests and
// binaries because proc-macro call sites are already explicit and stable.

pub mod net {
    pub use tokio::net::TcpListener;
}

pub mod process {
    pub use tokio::process::Command;
}

pub mod runtime {
    pub use tokio::runtime::{Builder, Handle, Runtime};
}

pub mod sync {
    pub use tokio::sync::{Mutex, MutexGuard, broadcast, mpsc, oneshot};
}

pub mod task {
    pub use tokio::task::{JoinSet, spawn_blocking};
}

pub mod time {
    pub use tokio::time::timeout;
}

pub use tokio::{select, signal, spawn};

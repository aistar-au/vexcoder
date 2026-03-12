use anyhow::Result;

use crate::runtime::CommandRequest;

pub trait SandboxDriver: Send + Sync {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PassthroughSandbox;

impl SandboxDriver for PassthroughSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        Ok(req)
    }
}

#[cfg(test)]
mod tests {
    use super::{PassthroughSandbox, SandboxDriver};
    use crate::runtime::CommandRequest;

    #[test]
    fn passthrough_sandbox_is_identity() {
        let req = CommandRequest {
            program: "echo".into(),
            args: vec!["hello".into()],
            working_dir: None,
        };
        let wrapped = PassthroughSandbox.wrap(req).expect("wrap request");
        assert_eq!(wrapped.program, "echo");
        assert_eq!(wrapped.args, vec!["hello"]);
    }
}

use {
    crate::{
        cli::{GlobalArgs, OutputFormat},
        runtime::{config::resolve_url, rpc::Rpc},
    },
    anyhow::Result,
};

pub(crate) struct CliContext {
    url: Option<String>,
    pub(crate) output: OutputFormat,
}

impl CliContext {
    pub(crate) fn new(globals: GlobalArgs) -> Self {
        Self {
            url: globals.url,
            output: globals.output,
        }
    }

    pub(crate) fn rpc(&self) -> Result<Rpc> {
        Ok(Rpc::new(resolve_url(self.url.as_deref())?))
    }
}

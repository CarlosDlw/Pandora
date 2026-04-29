//! Pluggable frontend so `crate::pipeline::Pipeline` can run the Pandora compiler (`core`)
//! without circular crate dependencies (`core` depends on `foundation`, not the reverse).

use crate::{
    diagnostics::Diagnostics,
    ids::FileId,
};

/// Compiles source for a [`FileId`]. Implemented in the `core` crate.
pub trait PandoraFrontend {
    fn compile_file(
        &mut self,
        file_id: FileId,
        source: &str,
        builtins: Option<&std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Diagnostics;
}

impl<F> PandoraFrontend for F
where
    F: FnMut(FileId, &str, Option<&std::sync::Arc<dyn std::any::Any + Send + Sync>>) -> Diagnostics,
{
    fn compile_file(
        &mut self,
        file_id: FileId,
        source: &str,
        builtins: Option<&std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Diagnostics {
        (*self)(file_id, source, builtins)
    }
}

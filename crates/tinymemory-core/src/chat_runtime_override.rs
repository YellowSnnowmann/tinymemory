//! Production runtime-override policy: no test provider is installed.

use super::*;

pub(super) fn current_runtime() -> Option<(Arc<dyn ChatProvider>, String)> {
    None
}

//! Historical per-category catalog module paths.
//!
//! Before OpenHuman#5560 moved the curated catalogs into the contract crate,
//! each category lived in its own `pub mod catalogs_<category>` here (e.g.
//! `providers::catalogs_business::SHOPIFY_CURATED`). The move consolidated
//! them into [`super::catalogs`], which flattens every constant to one level
//! (`providers::catalogs::SHOPIFY_CURATED`) rather than nesting them by
//! category — so the six original module names stopped resolving even though
//! [`super::catalogs`] kept every constant reachable under a different path.
//!
//! `AGENTS.md`'s SemVer policy treats a removed public path as a breaking
//! change unless the crate takes a major (pre-1.0: minor) bump for it. Rather
//! than force that bump for a rename, these six modules re-export the same
//! constants under their historical names — pure re-exports, no behavior, no
//! new dependency.
//!
//! # Deletion
//!
//! This module is a deprecation shim, not a permanent home. It may be deleted
//! in the next minor version bump that is *already* taking other breaking
//! changes (so the cost is paid once), or once nothing in this workspace or a
//! known downstream consumer (the OpenHuman host) still names a
//! `catalogs_<category>` path — check with
//! `grep -rn 'catalogs_business\|catalogs_google\|catalogs_messaging\|catalogs_microsoft\|catalogs_productivity\|catalogs_social_media'`
//! across both repositories before removing it.

pub mod catalogs_business {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::business::*;
}

pub mod catalogs_google {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::google::*;
}

pub mod catalogs_messaging {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::messaging::*;
}

pub mod catalogs_microsoft {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::microsoft::*;
}

pub mod catalogs_productivity {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::productivity::*;
}

pub mod catalogs_social_media {
    //! Historical compat shim — see the module docs above.
    pub use tinymemory_api::composio::catalogs::social_media::*;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_module_paths_resolve_to_the_same_constants_as_the_new_path() {
        assert_eq!(
            catalogs_business::SHOPIFY_CURATED.len(),
            crate::sync::composio::providers::catalogs::SHOPIFY_CURATED.len()
        );
        assert_eq!(
            catalogs_google::GOOGLEDRIVE_CURATED.len(),
            crate::sync::composio::providers::catalogs::GOOGLEDRIVE_CURATED.len()
        );
        assert_eq!(
            catalogs_messaging::SLACK_CURATED.len(),
            crate::sync::composio::providers::catalogs::SLACK_CURATED.len()
        );
        assert_eq!(
            catalogs_microsoft::EXCEL_CURATED.len(),
            crate::sync::composio::providers::catalogs::EXCEL_CURATED.len()
        );
        assert_eq!(
            catalogs_productivity::JIRA_CURATED.len(),
            crate::sync::composio::providers::catalogs::JIRA_CURATED.len()
        );
        assert_eq!(
            catalogs_social_media::TWITTER_CURATED.len(),
            crate::sync::composio::providers::catalogs::TWITTER_CURATED.len()
        );
    }
}

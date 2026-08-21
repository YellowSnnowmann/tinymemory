//! Document and URL intake for TinyMemory.
//!
//! Getting a PDF, a `.docx`, an HTML export, or a web page into memory is three
//! problems, and only the middle one is interesting:
//!
//! 1. **Work out what it is.** [`format::DocumentFormat::sniff`] reads magic
//!    bytes, the declared MIME type and the filename, in that order.
//! 2. **Turn it into markdown.** [`convert::DocumentConverter`] is the seam;
//!    [`convert::NativeConverter`] covers text, markdown and HTML with no
//!    dependencies, and a host binds its own for PDF and DOCX.
//! 3. **Put it in whichever engine is bound.** [`ingest::DocumentIntake`]
//!    picks the best family the driver actually implements — chunked ingest,
//!    the document tier, or the mandatory core — and reports which it used.
//!
//! Markdown is the intermediate form throughout: it is the one representation
//! that survives chunking, embedding, and being read back by a human.
//!
//! # Example
//!
//! ```
//! use tinymemory_documents::convert::{ConverterChain, DocumentConverter, RawDocument};
//!
//! # let runtime = tokio::runtime::Builder::new_current_thread().build()?;
//! # runtime.block_on(async {
//! let chain = ConverterChain::default();
//! let html = RawDocument::new("<h1>Notes</h1><p>A <b>point</b>.</p>")
//!     .with_mime("text/html")
//!     .with_filename("notes.html");
//!
//! let converted = chain.convert(&html).await?;
//! assert_eq!(converted.markdown, "# Notes\n\nA **point**.");
//! assert_eq!(converted.title, None);
//! # Ok::<(), tinymemory_api::error::MemoryError>(())
//! # })?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Feature flags
//!
//! - `network` — [`fetch::fetch_url`], the URL intake path. Off by default, so
//!   a host that only accepts uploads links no HTTP stack.

pub mod convert;
pub mod error;
#[cfg(feature = "network")]
pub mod fetch;
pub mod format;
pub mod html;
pub mod ingest;

pub use convert::{
    check_size, ConvertedDocument, ConverterChain, DocumentConverter, NativeConverter, RawDocument,
    MAX_DOCUMENT_BYTES,
};
pub use error::Result;
pub use format::DocumentFormat;
pub use ingest::{DocumentIntake, IntakeReceipt, IntakeRequest, IntakeRoute};

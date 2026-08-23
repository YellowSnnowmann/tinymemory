# tinymemory-documents

Document and URL intake for TinyMemory: work out what a file is, turn it into
markdown, and put it in whichever engine is bound.

## Why this is a crate and not a function

Because it is three separable decisions, and only the host can make two of
them.

**What the file is** is a detection problem with three unreliable signals.
`DocumentFormat::sniff` reads magic bytes first (the only signal a caller
cannot get wrong), then the declared MIME type, then the filename, then falls
back to looking at the bytes. A browser that sends
`application/octet-stream` for a PDF still gets a PDF.

**What it becomes** is markdown, always. It is the one representation that
survives every hop the content makes afterwards: chunkers split on its
headings, embedders read it as prose, and a human can read the stored copy
without a renderer. Converting to plain text would throw away the structure a
chunker needs; keeping the original bytes would push the problem onto every
engine separately.

**Where it lands** depends on the driver, and the contract offers three
answers. `DocumentIntake` picks the best one the bound driver actually
implements and reports which it used — so the same upload behaves the same way
against TinyCortex, Mem0 and a mandatory-only driver, and a host can see when a
document did *not* get chunked.

## Public surface

| Item | What it is |
| --- | --- |
| `DocumentFormat` | markdown / plain text / HTML / PDF / DOCX / unknown, and `sniff` |
| `RawDocument` | bytes plus filename, declared MIME, and origin |
| `ConvertedDocument` | markdown plus title, source format, and converter metadata |
| `DocumentConverter` | the conversion seam — object-safe and async |
| `NativeConverter` | text, markdown and HTML, with no dependencies |
| `ConverterChain` | converters in priority order; first claim wins |
| `DocumentIntake` | conversion plus the write, against a bound `MemoryProvider` |
| `IntakeRequest` / `IntakeReceipt` | where a document should go, and what happened |
| `fetch::fetch_url` | one URL, once, behind the shared SSRF guard (`network`) |
| `html::to_markdown` | the structural HTML converter, usable on its own |

## PDF and DOCX

Not handled here. Both need a real extractor, and which one a deployment uses
is its own decision — an in-process crate, a TinyBus module, a service. So
conversion is a trait a host binds:

```rust,ignore
let chain = ConverterChain::default().prepend(Box::new(MyPdfConverter));
```

A format nothing in the chain claims is rejected with an error naming the
format and listing what the build *can* convert. It is never a silent empty
document — storing an empty body loses the upload while looking like a success.

## Routing rules

| Driver implements | Route | What happens |
| --- | --- | --- |
| `MemoryIngest` | `ingest` | the driver chunks and embeds the markdown |
| `MemoryDocuments` | `documents` | stored whole, queryable by the document tier |
| neither | `core` | one entry through the mandatory family |

`DocumentIntake::route()` answers this without performing a write, so a host can
tell a user what will happen before it happens.

## Operational constraints

- **Taint is passed through, never assigned.** The contract is explicit that
  the host stamps provenance. `IntakeRequest` defaults to `ExternalSync` — the
  closed default — and a host that knows better sets it.
- **Size is capped before conversion.** `MAX_DOCUMENT_BYTES` (32 MiB) is
  checked on the raw bytes, because a document that would not fit is one this
  process should never finish decoding.
- **Keys are derived and stable.** The same URL or filename always produces the
  same key, so re-ingesting a document upserts instead of storing a second copy.
  URLs lose their scheme first, so `http://` and `https://` fetches of one page
  do not diverge.
- **The namespace is validated first.** Against the `tinymemory_api::namespace`
  convention, before any write, so a malformed namespace fails at the boundary
  rather than inside an engine.
- **URL fetches reuse the source readers' SSRF guard.** Two SSRF
  implementations in one workspace means one of them is the weaker, and nobody
  knows which.

## Features

- `network` — `fetch::fetch_url`. Off by default; a host that only accepts
  uploads links no HTTP stack.

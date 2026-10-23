# MIME preparation

The reusable `eml_core::prepare_message` capability prepares immutable bytes,
authorizes removal independently, and verifies exact expected bytes and retained
structure. Retained text/plain and text/html payloads are never decoded or
regenerated. Native execution and release acceptance require the separate
[verification gates](verification.md).

## Retained body and removal policy

Preparation creates body-only copies. All retained text/plain and
text/html representations remain in source order, including empty bodies and
their original encoded payload bytes. A filename does not decide whether a part
is body content.

An ordinary non-root entity with a valid disposition other than `inline` is
removed whole before its interior is parsed. This includes extension tokens
such as `x-unknown` and `form-data`; an absent disposition remains eligible
for body selection. Empty, malformed or duplicate primary controls refuse. A multipart/related container selects its direct body root
by `start`, or its first child when `start` is absent; every other direct child is
removed whole as a related resource. `multipart/report` reached in body context refuses the entire preparation
with `UNSUPPORTED_MIME`: its required report parts cannot be preserved by this
body-only policy. Attached reports and nonselected related reports stay opaque
and removable, including malformed interiors. Other generic multipart
containers classify all children in order. Identity-encoded message/rfc822 and message/global parts are
descended using the embedded root's own rules; encoded or opaque forwarded
messages follow the non-body rule.

Every other leaf is removed as a non-body part, including inline images outside
related containers, text/calendar and delivery-status parts. A container without
a retained body descendant collapses. Removal metadata describes the ordered
decisions, so its byte totals need not equal the actual source-minus-copy saving.
At least one supported text body must remain, or preparation returns `NoBody`
without a copy.

Signed/encrypted and protected signature/control content reached in body context
is refused before descent. An attached protected entity or a protected entity
inside an already removed related resource can instead be removed whole. A
disposition on a top-level or embedded message root does not make that root an
ordinary attachment.

Stale length/checksum fields are removed from changed retained ancestors, and
stale authentication fields from changed message roots. Unchanged siblings retain
their fields. Independent source authorization and byte/structure verification
must accept every prepared result. Retained HTML, remote references and mislabeled
data inside text bodies are not sanitized; an attachment-free result is not a
malware or safe-rendering assessment.

## Unquoted parameter whitespace and comments

Comments in structured MIME control fields are CFWS, including in unquoted
parameter values. Outside CFWS is removed. Internal literal spaces and tabs are
preserved; if an internal run contains only comments, one space separates the
data fragments. Thus `boundary=B (tail)` means B, while B(comment)C means B C
and cannot match delimiter BC. Comment text is never boundary or filename data.
An internal tab is retained lexically and then fails structural printable-ASCII
validation. Invalid structural comments are refused; invalid display values are
ignored. The bounded comment-depth limit still produces LimitExceeded.

Quoted strings and RFC 2231 percent-decoded values are not scanned again for
comments. A quoted `"B (tail)"` or decoded B%28tail%29 can contain literal
parentheses. These rules preserve the deliberate unquoted-space/8-bit lenience
without allowing a comment to join two token fragments. They affect interpreted
controls and display metadata; no source body bytes are rewritten.

The structured-field convention is described by
[RFC 2045 §5.1](https://www.rfc-editor.org/rfc/rfc2045#section-5.1).

## Display encoded-word tolerance

Filename metadata tolerates valid RFC 2047 B/Q word syntax even though RFC 2047
formally excludes encoded words in MIME parameters. This is display tolerance,
not a claim of whole-message RFC compliance. A word is at most 75 ASCII bytes;
encoded-text excludes raw spaces, tabs and question marks. Q hex escapes and
underscore-as-space are decoded. Invalid or undecodable words remain literal.
Whitespace is suppressed only between two successfully decoded adjacent words,
including folded linear whitespace after header unfolding. The scanner's
lookahead is bounded even for long repeated unterminated prefixes.

## Preparation work and cancellation

One cached physical cursor scans the message forward. A stack-scoped trie matches
active boundaries at physical line starts, giving the outermost matching owner
precedence. Explicit parser frames retain ancestor visibility through forwarded
messages and opaque entities. The terminator before a delimiter belongs to the
parent frame; header charges and spans exclude it.

Authorization independently derives retained nodes and ordered removal claims.
A forward source-span iterator emits each retained byte once without rebuilding
payloads at container ancestors. Preparation allocates one candidate payload,
compares every authorized byte, reparses and compares the retained structure,
then proves policy fixed-point coverage without constructing another payload.

Whole-message scanning, pre-admission header classification, copying and equality
comparisons checkpoint in units of at most 64 KiB. Admitted header metadata and
charset decoding have separate existing size bounds; allocation, scheduling and
native I/O prevent a universal elapsed-time stop guarantee. Test-only counters
measure actual scan/classification/padding/copy/comparison work and inject stop
at each preparation phase. These tests do not replace the required controlled
Linux release-build timing and RSS campaign.

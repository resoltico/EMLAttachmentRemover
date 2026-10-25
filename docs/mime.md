# MIME preparation

The reusable `eml_message::prepare_message` capability prepares immutable bytes,
authorizes removal independently, and verifies exact expected bytes and retained
structure. Retained text/plain and text/html payloads are never decoded or
regenerated. Filesystem execution is separate; see the [architecture](architecture.md).

## Retained body and removal policy

Preparation creates body-only copies. All retained text/plain and
text/html representations remain in source order, including empty bodies and
their original encoded payload bytes. A filename does not decide whether a part
is body content.

An ordinary non-root entity with a present non-inline disposition, including a valid
extension token such as `x-unknown`, is removed whole before
its interior is parsed. A multipart/related container selects its direct body root
by `start`, or its first child when `start` is absent; every other direct child is
removed whole as a related resource. Other multipart containers classify all
children in order. Identity-encoded message/rfc822 and message/global parts are
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

Retained-body `multipart/report` is refused with `UNSUPPORTED_MIME`: a body-only
copy cannot preserve its required machine-readable parts. Attached reports and
nonselected related reports remain opaque removable entities.

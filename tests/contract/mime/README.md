# MIME field regressions

Inputs 01–14 and the disposition corpus retain their existing contracts.

| Input | Expected result |
|---|---|
| 15-mailbox-export | MAILBOX_FILE; no copy |
| 16-content-after-end | Exact golden; attachment and after_message_end removals |
| 17-leading-bom | MALFORMED; no copy |
| 18-incomplete-multipart | MALFORMED; damaged-email explanation; no copy |
| 19-mixed-line-ending-epilogue | Exact golden; one 2-byte after_message_end removal; enclosing framing unchanged |

Preambles remain unchanged. Single-message From envelopes remain supported.

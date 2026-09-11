# Draft-file read timeout

The pinned upstream worksheet limits asynchronous draft-file reads to five seconds, in addition to the existing 1 MiB input cap. Timeout preserves current answers and restores editing/export controls. An old read completing after timeout cannot replace a newer imported packet. This is not cancellation of the underlying browser/OS read or protection against arbitrary synchronous JavaScript.

The dedicated real-server Chromium test stalls the File API, observes timeout recovery, exports the preserved answers, imports a newer packet, then releases the old read and verifies no overwrite, approval dialog or late error occurs. It is included in the existing blocking browser test command with all prior cases retained. No new package dependency or CI bypass is introduced.

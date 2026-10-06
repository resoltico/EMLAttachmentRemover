# Runtime notice supplements

These are unmodified third-party notices, not project artwork or MPL-licensed project code. The pinned python-build-standalone 20261003 macOS archives declare these notice paths in `PYTHON.json` but omit their texts. Packaging supplies the missing files without replacing notices already supplied upstream, then requires every metadata-referenced notice to exist as an ordinary nonempty file.

- [Zstandard BSD notice](LICENSE.zstd.txt): copied from [CPython's zstd-1.5.7 dependency snapshot](https://github.com/python/cpython-source-deps/blob/zstd-1.5.7/LICENSE); the bundled extension reports Zstandard 1.5.7.
- [zlib-ng notice](LICENSE.zlib-ng.txt): copied from [CPython's zlib-ng-2.2.4 dependency snapshot](https://github.com/python/cpython-source-deps/blob/zlib-ng-2.2.4/LICENSE.md). The macOS runtime uses system zlib; this notice fulfills the additional reference retained in upstream metadata, without claiming that zlib-ng is bundled.

The dependency versions and notice filenames come from [the standalone runtime's immutable build configuration](https://github.com/astral-sh/python-build-standalone/blob/20261003/pythonbuild/downloads.json). Recheck these supplements when changing the runtime pin. Original CPython, OpenSSL and other upstream notices remain in the runtime tree; package-management and Tcl/Tk code is omitted from processor runtimes while upstream notices remain. OpenSSL 3.5.9 supplies `LICENSE.txt` and no root NOTICE document; no fabricated NOTICE text is added.

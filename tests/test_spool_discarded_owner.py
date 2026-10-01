"""Secondary Python disposal closes discarded owners without relying on path cleanup."""

from __future__ import annotations

import gc

from eml_attachment_remover.report_spool import ReportSpool


def test_discarded_owner_closes_the_open_stream() -> None:
    spool = ReportSpool.create()
    owned = spool.file
    del spool
    gc.collect()
    assert owned.closed


def test_interrupted_construction_and_failed_disposal_are_safe() -> None:
    incomplete = ReportSpool.__new__(ReportSpool)
    del incomplete
    gc.collect()

    class Broken:
        @staticmethod
        def close() -> None:
            message = "public discard close failure"
            raise OSError(message)

    broken = ReportSpool(Broken())  # type: ignore[arg-type]
    del broken
    gc.collect()

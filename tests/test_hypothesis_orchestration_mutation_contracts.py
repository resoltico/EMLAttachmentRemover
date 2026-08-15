"""Mutation-resistant contracts for isolated Hypothesis orchestration."""

from __future__ import annotations

import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from tools import hypothesis_publication, hypothesis_run


class ObservationPublicationMutationContracts(unittest.TestCase):
    """Pin canonical observation paths, labels, and copy behavior."""

    def test_publication_uses_exact_sanitizer_and_filesystem_contract(self) -> None:
        with (
            tempfile.TemporaryDirectory() as source_directory,
            tempfile.TemporaryDirectory() as publication_directory,
        ):
            source = Path(source_directory).resolve()
            publication = Path(publication_directory).resolve()
            source_observations = source / ".hypothesis" / "observed"
            source_observations.mkdir(parents=True)
            (source_observations / "public.jsonl").write_text(
                "{}\n",
                encoding="utf-8",
            )
            with (
                patch.object(
                    hypothesis_publication.artifacts,
                    "finalize",
                    return_value=(1, 2),
                ) as finalize,
                patch.object(
                    hypothesis_publication,
                    "_directory_exists",
                    wraps=hypothesis_publication._directory_exists,  # ruff: ignore[private-member-access]
                ) as exists,
                patch.object(shutil, "copytree", wraps=shutil.copytree) as copytree,
            ):
                result = hypothesis_publication.publish_observations(
                    source,
                    publication,
                )
                copy_source, staged = copytree.call_args.args
                destination = publication / ".hypothesis" / "observed"
                published_content = (destination / "public.jsonl").read_text()
                staging_exists = staged.parent.exists()

        self.assertEqual(result, (1, 2))
        finalize.assert_called_once_with(
            source,
            observations=True,
            additional_replacements=((publication, "<project-root>"),),
        )
        public_root = publication / ".hypothesis"
        destination = public_root / "observed"
        self.assertEqual(
            exists.call_args_list,
            [
                call(public_root, "public Hypothesis root"),
                call(destination, "public Hypothesis observations"),
            ],
        )
        copytree.assert_called_once()
        self.assertEqual(copy_source, source_observations)
        self.assertEqual(staged.name, "observed")
        self.assertTrue(staged.parent.name.startswith(".observed-publication-"))
        self.assertEqual(
            copytree.call_args.kwargs,
            {"copy_function": shutil.copyfile},
        )
        self.assertEqual(published_content, "{}\n")
        self.assertFalse(staging_exists)

    def test_empty_observation_error_is_exact(self) -> None:
        with (
            patch.object(
                hypothesis_publication.artifacts,
                "finalize",
                return_value=(0, 0),
            ),
            self.assertRaises(
                hypothesis_publication.HypothesisPublicationError
            ) as raised,
        ):
            hypothesis_publication.publish_observations(
                Path("source"),
                Path("publication"),
            )
        self.assertEqual(
            str(raised.exception),
            "observable Hypothesis run produced no observation files",
        )


class HypothesisRunnerMutationContracts(unittest.TestCase):
    """Pin cleanup, permission, report, and grouped-failure behavior."""

    def test_failed_initialization_uses_nonmasking_best_effort_cleanup(self) -> None:
        owned = Path(tempfile.mkdtemp(prefix=hypothesis_run.STORAGE_PREFIX)).resolve()
        initialization_error = ValueError("initialization failed")
        try:
            with (
                patch.object(tempfile, "mkdtemp", return_value=str(owned)),
                patch.object(
                    hypothesis_run,
                    "_initialize_storage",
                    side_effect=initialization_error,
                ),
                patch.object(shutil, "rmtree") as remove,
                self.assertRaises(ValueError) as raised,
            ):
                hypothesis_run._create_storage(  # ruff: ignore[private-member-access]
                    Path.cwd()
                )
            self.assertIs(raised.exception, initialization_error)
            remove.assert_called_once_with(owned, ignore_errors=True)
        finally:
            shutil.rmtree(owned, ignore_errors=True)

    def test_storage_creation_uses_exact_name_and_private_mode(self) -> None:
        root = Path(tempfile.mkdtemp(prefix=hypothesis_run.STORAGE_PREFIX)).resolve()
        real_mkdir = Path.mkdir

        def mkdir(path: Path, *args: object, **kwargs: object) -> None:
            real_mkdir(path, *args, **kwargs)  # type: ignore[arg-type]

        try:
            with (
                patch.object(
                    hypothesis_run,
                    "_is_owned_temporary_root",
                    return_value=True,
                ),
                patch.object(Path, "mkdir", autospec=True, side_effect=mkdir) as make,
            ):
                storage = hypothesis_run._initialize_storage(  # ruff: ignore[private-member-access]
                    root,
                    Path.cwd(),
                )
            self.assertEqual(storage, root / ".hypothesis")
            make.assert_called_once_with(storage, mode=0o700)
        finally:
            shutil.rmtree(root, ignore_errors=True)

    def test_storage_initialization_rejects_a_canonical_named_regular_file(
        self,
    ) -> None:
        with tempfile.NamedTemporaryFile(
            prefix=hypothesis_run.STORAGE_PREFIX,
            delete=False,
        ) as temporary:
            root = Path(temporary.name).resolve()
        try:
            with self.assertRaises(hypothesis_run.HypothesisRunError) as raised:
                hypothesis_run._initialize_storage(  # ruff: ignore[private-member-access]
                    root,
                    Path.cwd(),
                )
            self.assertEqual(
                str(raised.exception),
                f"Hypothesis temporary storage is unsafe: {root}",
            )
        finally:
            root.unlink(missing_ok=True)

    def test_windows_accepts_acl_managed_mode_while_posix_rejects_it(self) -> None:
        root = Path(tempfile.mkdtemp(prefix=hypothesis_run.STORAGE_PREFIX)).resolve()
        root.chmod(0o755)
        try:
            with (
                patch.object(
                    hypothesis_run,
                    "_is_owned_temporary_root",
                    return_value=True,
                ),
                patch("tools.hypothesis_run.os.name", "nt"),
            ):
                storage = hypothesis_run._initialize_storage(  # ruff: ignore[private-member-access]
                    root,
                    Path.cwd(),
                )
            self.assertTrue(storage.is_dir())
            shutil.rmtree(storage)
            with (
                patch.object(
                    hypothesis_run,
                    "_is_owned_temporary_root",
                    return_value=True,
                ),
                patch("tools.hypothesis_run.os.name", "posix"),
                self.assertRaisesRegex(
                    hypothesis_run.HypothesisRunError,
                    "temporary storage is unsafe",
                ),
            ):
                hypothesis_run._initialize_storage(  # ruff: ignore[private-member-access]
                    root,
                    Path.cwd(),
                )
        finally:
            shutil.rmtree(root, ignore_errors=True)

    def test_report_publication_uses_canonical_private_source(self) -> None:
        failures: list[Exception] = []
        cleanup_root = Path("isolated-root")
        project_root = Path("project-root")
        destination = Path("project-root/build/public.xml")
        with patch.object(hypothesis_run.junit_report, "publish") as publish:
            hypothesis_run._publish_report(  # ruff: ignore[private-member-access]
                cleanup_root,
                project_root,
                destination,
                failures,
            )
        publish.assert_called_once_with(
            cleanup_root / "test-results.xml",
            destination,
            project_root,
        )
        self.assertEqual(failures, [])

    def test_grouped_finalization_message_is_exact(self) -> None:
        first = ValueError("publication failed")
        second = hypothesis_run.HypothesisRunError("cleanup failed")
        real_cleanup = hypothesis_run._cleanup  # ruff: ignore[private-member-access]

        def clean_then_fail(root: Path) -> None:
            real_cleanup(root)
            raise second

        with (
            patch.object(
                hypothesis_run.artifacts,
                "finalize",
                side_effect=first,
            ),
            patch.object(hypothesis_run, "_cleanup", side_effect=clean_then_fail),
            self.assertRaises(ExceptionGroup) as raised,
        ):
            hypothesis_run.run_isolated(lambda _storage: None, Path.cwd())
        self.assertEqual(
            str(raised.exception).split(" (2 sub-exceptions)", maxsplit=1)[0],
            "Hypothesis run finalization failed",
        )
        self.assertEqual(raised.exception.exceptions, (first, second))


if __name__ == "__main__":
    unittest.main(verbosity=2)

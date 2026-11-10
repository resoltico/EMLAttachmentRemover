"""The sync guard permits product data while rejecting engine changes."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("check_sync", Path(__file__).with_name("check-sync.py"))
check_sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_sync)


class EngineComparison(unittest.TestCase):
    def test_product_literals_and_crlf_do_not_change_engine(self):
        first = b"# BEGIN PRODUCT SETTINGS\ncommand_name='pdfconcat'\narchive_rooted=0\n# END PRODUCT SETTINGS\nprintf same\n"
        other = first.replace(b"pdfconcat", b"emlattachmentremover").replace(b"\n", b"\r\n")
        self.assertEqual(check_sync.engine(first, "install.sh"), check_sync.engine(other, "install.sh"))
        changed = other.replace(b"printf same", b"printf different")
        self.assertNotEqual(check_sync.engine(first, "install.sh"), check_sync.engine(changed, "install.sh"))

    def test_executable_or_unbounded_product_blocks_are_rejected(self):
        for data in [
            b"# BEGIN PRODUCT SETTINGS\ncommand_name=$(bad)\n# END PRODUCT SETTINGS\n",
            b"# BEGIN PRODUCT SETTINGS\nfunction hidden() { bad; }\n# END PRODUCT SETTINGS\n",
            b"# BEGIN PRODUCT SETTINGS\ncommand_name='x'\n",
            b"# BEGIN PRODUCT SETTINGS\n# BEGIN PRODUCT SETTINGS\n# END PRODUCT SETTINGS\n",
        ]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                check_sync.engine(data, "install.sh")

    def test_powershell_literals_are_data(self):
        first = b"# BEGIN PRODUCT SETTINGS\n$product = @{\n Name = 'PDFConcat'\n LegalFiles = @('LICENSE', 'NOTICE')\n VersionedArchive = $false\n RootedArchive = $false\n}\n# END PRODUCT SETTINGS\nInvoke-Installer\n"
        second = first.replace(b"PDFConcat", b"EML Attachment Remover")
        self.assertEqual(check_sync.engine(first, "install.ps1"), check_sync.engine(second, "install.ps1"))
        with self.assertRaises(ValueError):
            check_sync.engine(first.replace(b"'PDFConcat'", b"(Invoke-Bad)"), "install.ps1")


if __name__ == "__main__":
    unittest.main()

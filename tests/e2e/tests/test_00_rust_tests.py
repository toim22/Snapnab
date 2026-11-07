"""The repository's own tests (`cargo test --workspace` in rust/): the .snap container and documents, XML, image files,
GIF, WebP, effects and Beautify against the former C++ results, barcodes, OCR, colors, the network code, the Editor's
painters and objects, video cuts, the translation catalogs and more (docs/ARCHITECTURE.md, Tests)."""
import shutil
import subprocess

import pytest

from snapnab_e2e.config import ARTIFACTS, RUST_DIR


@pytest.mark.slow
@pytest.mark.timeout(1800)
def test_rust_workspace_tests():
    cargo = shutil.which("cargo")
    if not cargo:
        pytest.skip("cargo not found (install Rust to run the workspace tests)")
    r = subprocess.run([cargo, "test", "--locked", "--release", "--workspace"], capture_output=True, timeout=1780,
                       cwd=RUST_DIR)
    out = r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")
    (ARTIFACTS / "cargo-test.log").write_text(out, encoding="utf-8")
    failed = [l for l in out.splitlines() if " FAILED" in l or l.startswith("error")]
    assert r.returncode == 0, f"exit {r.returncode}; failures:\n" + "\n".join(failed[:30])

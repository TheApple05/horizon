"""Build and freeze exact candidate binaries before runtime gates can use them."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    subprocess.run(["cargo", "build", "--locked", "--profile", "fast", "-p", "horizon-client",
                    "-p", "horizon-client-lib", "--bins"], cwd=args.candidate, check=True)
    target = Path(os.environ.get("CARGO_TARGET_DIR", args.candidate / "target"))
    args.output.mkdir(parents=True, exist_ok=False)
    suffix = ".exe" if os.name == "nt" else ""
    proof = {}
    for name in ("horizon-client", "horizon-ctl"):
        dest = args.output / (name + suffix)
        shutil.copy2(target / "fast" / dest.name, dest)
        with dest.open("rb") as source:
            proof[dest.name] = hashlib.file_digest(source, "sha256").hexdigest()
    proof["candidate"] = subprocess.check_output(["git", "-C", str(args.candidate), "rev-parse", "HEAD"], text=True).strip()
    (args.output / "proof.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof))


if __name__ == "__main__":
    main()

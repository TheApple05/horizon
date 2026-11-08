"""Package the offline client, static setup tools and original exporter support only."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--support", type=Path, required=True)
    parser.add_argument("--bootstrap", type=Path, required=True)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--private-marker", action="append", default=[])
    args = parser.parse_args()
    folder = Path("dist/Horizon-Offline-Auto")
    folder.mkdir(parents=True, exist_ok=True)
    files = []
    for path in args.support.rglob("*"):
        if not path.is_file():
            continue
        name = path.relative_to(args.support).as_posix()
        allowed = (name in ["horizon-prepare.exe", "research-lock.json",
                           "crates/horizon-prepare/src/exporters.rs"]
                   or (name.startswith("mods/horizon/src/test/java/") and name.endswith(".java"))
                   or (name.startswith(("exporters/server/dev/horizonlab/parity/",
                                        "exporters/client/dev/horizonlab/parity/")) and name.endswith(".class"))
                   or name == "exporters/manifest.json")
        assert allowed and not path.is_symlink(), f"Unexpected support file: {name}"
        target = folder / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        files.append(name)
    for source, name in [(args.bootstrap, "horizon-offline.exe"),
                         (args.client, "horizon-client.exe"),
                         (Path("packaging/offline/Start-Horizon-Offline.cmd"), "Start-Horizon-Offline.cmd"),
                         (Path("packaging/offline/README.txt"), "README.txt"),
                         (Path("LICENSE"), "LICENSE")]:
        shutil.copy2(source, folder / name)
        files.append(name)
    patch = "offline-private.patch"
    subprocess.run(["git", "diff", "upstream/main", "--binary", "--output=" + str(folder / patch)], check=True)
    files.append(patch)
    needles = [str(Path.home()), str(Path.home()).replace("\\", "/")] + args.private_marker
    if os.environ.get("COMPUTERNAME"):
        needles.append(os.environ["COMPUTERNAME"])
    sums = []
    for name in sorted(files):
        data = (folder / name).read_bytes()
        for needle in needles:
            for encoding in ["utf-8", "utf-16-le"]:
                assert needle.lower().encode(encoding) not in data.lower(), f"Personal identifier in {name}"
        sums.append(f"{hashlib.sha256(data).hexdigest()}  {name}\n")
    (folder / "SHA256SUMS.txt").write_text("".join(sums))
    files.append("SHA256SUMS.txt")
    archive = Path("dist/Horizon-Offline-Auto-Windows.zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=5) as z:
        for name in sorted(files):
            z.write(folder / name, "Horizon-Offline/" + name)
    with zipfile.ZipFile(archive) as z:
        assert z.testzip() is None
        assert len(z.namelist()) == len(set(files))
    Path("dist/SHA256SUMS-Auto.txt").write_text(
        f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
    print(f"Packaged {len(files)} files: {archive} ({archive.stat().st_size} bytes)")


if __name__ == "__main__":
    main()

"""Collect installed, locked dependency notices without adding a build dependency.

Includes Windows production/build Cargo dependencies and installed npm packages
(including build tools). MPL-only Rust crates include their corresponding source.
No application data directories are accessed.
"""
import json
import shutil
import subprocess
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "src-tauri/target/preview-resources"


def text(path):
    return path.read_text(encoding="utf-8-sig", errors="replace")


def license_files(directory):
    return sorted(p for p in directory.iterdir() if p.is_file() and
                  p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    # Only deterministic generated files are overwritten; never read AppData.
    shutil.copyfile(ROOT / "LICENSE", OUT / "LICENSE.txt")
    shutil.copytree(ROOT / "licenses/fonts", OUT / "fonts", dirs_exist_ok=True)
    shutil.copytree(ROOT / "licenses/installer", OUT / "installer", dirs_exist_ok=True)
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
        "--format-version", "1", "--locked", "--offline",
        "--filter-platform", "x86_64-pc-windows-msvc",
    ], cwd=ROOT))
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    reachable, pending = set(), [metadata["resolve"]["root"]]
    while pending:
        key = pending.pop()
        if key in reachable:
            continue
        reachable.add(key)
        pending.extend(d["pkg"] for d in nodes[key]["deps"]
                       if any(k["kind"] != "dev" for k in d["dep_kinds"]))
    inventory, notices, missing = [], [], []
    supplements = ROOT / "licenses/dependencies"
    supplement_sources = json.loads(text(supplements / "sources.json"))
    aliases = {
        "dasp_sample": "dasp_sample", "tauri-plugin": "tauri",
        "@tauri-apps/cli-win32-x64-msvc": "tauri", "@polka/url": "polka",
        "@rolldown/binding-win32-x64-msvc": "rolldown",
        "@sqlite.org/sqlite-wasm": "sqlite-wasm",
    }

    def record(kind, name, version, license_id, repository, directory, files):
        entry = dict(ecosystem=kind, name=name, version=version,
                     license=license_id, repository=repository)
        inventory.append(entry)
        notices.append(f"\n{'=' * 72}\n{kind}: {name} {version}\n"
                       f"License: {license_id}\nSource: {repository}\n")
        if not files:
            key = aliases.get(name, name)
            if "MPL" in license_id:
                key = "mpl-2.0"
            elif name.startswith("unic-"):
                key = "unic"
            elif name.startswith("webview2-com"):
                key = "webview2"
            supplement = supplements / (key + ".txt")
            if supplement.is_file():
                notices.append(f"Supplement source: {supplement_sources.get(key, repository)}\n"
                               f"{text(supplement)}\n")
            else:
                missing.append(f"{kind}: {name}@{version} ({license_id})")
        for file in files:
            notices.append(f"\n--- {file.relative_to(directory)} ---\n{text(file)}\n")

    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in reachable or not package["source"]:
            continue
        directory = Path(package["manifest_path"]).parent
        files = license_files(directory)
        if package.get("license_file"):
            extra = directory / package["license_file"]
            if extra.is_file() and extra not in files:
                files.append(extra)
        license_id = package.get("license") or "See included license"
        record("Cargo", package["name"], package["version"], license_id,
               package.get("repository") or "https://crates.io/crates/" + package["name"], directory, files)
        if "MPL" in license_id and " OR " not in license_id:
            archive = OUT / f"source-{package['name']}-{package['version']}.zip"
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, strict_timestamps=False) as output:
                for source in sorted(directory.rglob("*")):
                    if source.is_file() and source.name not in (".cargo-ok",):
                        output.write(source, f"{package['name']}-{package['version']}/{source.relative_to(directory).as_posix()}")

    lock = json.loads(text(ROOT / "package-lock.json"))
    for relative in sorted(lock["packages"]):
        if not relative:
            continue
        directory = ROOT / relative
        manifest = directory / "package.json"
        if not manifest.is_file():  # optional dependency for another platform
            continue
        package = json.loads(text(manifest))
        repository = package.get("repository", "https://www.npmjs.com/package/" + package["name"])
        if isinstance(repository, dict):
            repository = repository.get("url", "")
        record("npm (includes build tools)", package["name"], package["version"],
               package.get("license", "See included license"), repository, directory, license_files(directory))

    (OUT / "THIRD-PARTY-NOTICES.txt").write_text(
        "Pomotroid Fork Preview — third-party notices\n"
        "Original Pomotroid: Copyright (c) 2018 Christopher Murphy. MIT.\n"
        "The unchanged project license is LICENSE.txt.\n"
        "Bundled Mona Sans fonts: see fonts/.\n"
        "NSIS and its Tauri utility plugin: see installer/.\n"
        "Windows Cargo production/build dependencies and installed npm build tools follow.\n"
        "Corresponding MPL-only Rust crate sources are included as source-*.zip.\n"
        "Package registries/source repositories above provide other source code.\n" + "".join(notices), encoding="utf-8")
    (OUT / "dependency-inventory.json").write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    # Missing texts must be resolved before an installer is produced.
    if missing:
        raise SystemExit("Missing license text:\n" + "\n".join(missing))
    print(f"Collected notices for {len(inventory)} dependency entries into {OUT}")


if __name__ == "__main__":
    main()

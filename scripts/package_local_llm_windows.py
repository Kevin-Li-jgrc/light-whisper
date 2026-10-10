"""在非同步目录用 Tauri 生成的 NSIS 脚本打包；不安装或修改用户配置。"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def main():
    nsis = ROOT / 'src-tauri/target/release/nsis/x64'
    source = (nsis / 'installer.nsi').read_text(encoding='utf-8-sig')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    stage = Path(os.environ['LOCALAPPDATA']) / 'LightWhisperBuild' / ('local-ai-' + stamp)
    mirror = stage / 'source'
    build = stage / 'nsis'
    build.mkdir(parents=True)
    files = {}
    for value in re.findall(r'"([A-Za-z]:\\[^"\r\n]+)"', source):
        path = Path(value).resolve()
        if path.is_file() and path.is_relative_to(ROOT):
            relative = path.relative_to(ROOT)
            target = mirror / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            # 写入新文件，避免继承同步目录的复制状态及属性。
            with path.open('rb') as src, target.open('wb') as dst:
                shutil.copyfileobj(src, dst, 1024 * 1024)
            files[relative.as_posix()] = {'size': target.stat().st_size, 'sha256': sha(target)}
            if files[relative.as_posix()]['sha256'] != sha(path):
                raise RuntimeError('Staging checksum mismatch: ' + str(relative))
    for path in nsis.iterdir():
        if path.is_file() and path.suffix in ['.nsh', '.nsi']:
            shutil.copy2(path, build / path.name)
    source = source.replace(str(ROOT), str(mirror))
    # 本机 NSIS 的未压缩 File 路径报错；zlib 最小复现可正常读取同一文件。
    source = source.replace('SetCompress off', 'SetCompress auto')
    source = source.replace('SetCompressor /SOLID "zlib"', 'SetCompressor zlib')
    (build / 'installer.nsi').write_text(source, encoding='utf-8')
    exe = Path(os.environ['LOCALAPPDATA']) / 'tauri/NSIS/makensis.exe'
    subprocess.run([str(exe), '/INPUTCHARSET', 'UTF8', '/V3', str(build / 'installer.nsi')], cwd=build, check=True)
    output = ROOT / 'build/local-ai-delivery'
    output.mkdir(parents=True, exist_ok=True)
    installer = output / 'Light-Whisper-1.5.9-local-ai-x64-setup.exe'
    shutil.copy2(build / 'nsis-output.exe', installer)
    manifest = {'installer': installer.name, 'size': installer.stat().st_size, 'sha256': sha(installer), 'stagingDirectory': str(stage), 'inputs': files}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    print(json.dumps({k: v for k, v in manifest.items() if k != 'inputs'}), flush=True)

if __name__ == '__main__':
    main()

"""准备固定版本的本地文字推理组件；不修改用户应用配置。"""
import hashlib
import json
import shutil
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / 'build' / 'local-llm'
DEST = ROOT / 'src-tauri' / 'resources' / 'local-llm'
TAG = 'b11118'
ARCHIVE_HASHES = {
    'llama-b11118-bin-win-cpu-x64.zip': '7f8431c69471cf8991f43da4af6e80f3a66778a63055004a9211ebba00d68084',
    'llama-b11118-bin-win-cuda-12.4-x64.zip': '09d262cb22c26d276a8c2cd38e0ddb405404c4c60e355c2ed96cda8e77538c59',
    'cudart-llama-bin-win-cuda-12.4-x64.zip': '8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6',
}
MODELS = [
    ('qwen3.5-0.8b', 'unsloth/Qwen3.5-0.8B-GGUF', '6ab461498e2023f6e3c1baea90a8f0fe38ab64d0', 'Qwen3.5-0.8B-Q8_0.gguf', 811843840, '0ad885ffd4bb022fc4f0d33a3308fa108ef8613159d3b3a67e23abca056b7a6c'),
    ('lfm2.5-1.2b', 'LiquidAI/LFM2.5-1.2B-Instruct-GGUF', '8ed288026e23958ad9dfa92d53ed773a8eee7125', 'LFM2.5-1.2B-Instruct-Q8_0.gguf', 1246253888, 'f6b981dcb86917fa463f78a362320bd5e2dc45445df147287eedb85e5a30d26a'),
]

def get(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent': 'Light-Whisper-build'}), timeout=120)

def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

def download(url, path, expected=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and (expected is None or digest(path) == expected):
        return
    partial = path.with_suffix(path.suffix + '.part')
    offset = partial.stat().st_size if partial.exists() else 0
    request = urllib.request.Request(url, headers={'User-Agent': 'Light-Whisper-build', **({'Range': f'bytes={offset}-'} if offset else {})})
    with urllib.request.urlopen(request, timeout=120) as r:
        append = offset > 0 and r.status == 206
        with partial.open('ab' if append else 'wb') as f:
            shutil.copyfileobj(r, f, 1024 * 1024)
    if expected and digest(partial) != expected:
        raise RuntimeError(f'Checksum mismatch: {path.name}')
    partial.replace(path)
    print('Downloaded', path.name, path.stat().st_size, flush=True)

def main():
    CACHE.mkdir(parents=True, exist_ok=True)
    release = json.load(get(f'https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/{TAG}'))
    names = {'cpu': f'llama-{TAG}-bin-win-cpu-x64.zip', 'cuda': f'llama-{TAG}-bin-win-cuda-12.4-x64.zip', 'cudart': 'cudart-llama-bin-win-cuda-12.4-x64.zip'}
    manifest = {'version': TAG, 'archives': [], 'models': []}
    for backend, name in names.items():
        asset = next(a for a in release['assets'] if a['name'] == name)
        path = CACHE / name
        expected = ARCHIVE_HASHES[name]
        download(asset['browser_download_url'], path, expected)
        target = DEST / ('cuda' if backend == 'cudart' else backend)
        target.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(path) as archive:
            for entry in archive.infolist():
                if entry.is_dir():
                    continue
                filename = Path(entry.filename).name
                if filename.endswith('.dll') or filename == 'llama-server.exe' or filename.startswith('LICENSE'):
                    with archive.open(entry) as source, (target / filename).open('wb') as output:
                        shutil.copyfileobj(source, output)
        manifest['archives'].append({'name': name, 'sha256': digest(path), 'size': path.stat().st_size})
    for model_id, repo, revision, filename, size, sha in MODELS:
        path = CACHE / 'models' / filename
        download(f'https://huggingface.co/{repo}/resolve/{revision}/{filename}', path, sha)
        if path.stat().st_size != size:
            raise RuntimeError('Unexpected model size')
        manifest['models'].append({'id': model_id, 'repo': repo, 'revision': revision, 'file': filename, 'size': size, 'sha256': sha})
    for name, url in {
        'llama-LICENSE.txt': f'https://raw.githubusercontent.com/ggml-org/llama.cpp/{TAG}/LICENSE',
        'Qwen-LICENSE.txt': 'https://huggingface.co/Qwen/Qwen3.5-0.8B/raw/main/LICENSE',
        'LFM-LICENSE.txt': 'https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct/raw/main/LICENSE',
        'NVIDIA-CUDA-12.4-EULA.html': 'https://docs.nvidia.com/cuda/archive/12.4.0/eula/index.html',
    }.items():
        download(url, DEST / name)
    manifest['runtimeFiles'] = [{'path': p.relative_to(DEST).as_posix(), 'sha256': digest(p)} for p in DEST.rglob('*') if p.is_file() and p.suffix in {'.exe', '.dll'}]
    (DEST / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    print('Provisioning complete', flush=True)

if __name__ == '__main__':
    main()

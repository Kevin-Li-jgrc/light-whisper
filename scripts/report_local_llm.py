"""汇总合成样例的真实输出；不把结构有效率当作语义正确率。"""
from pathlib import Path
import html
import json
import math
import statistics

ROOT = Path(__file__).resolve().parents[1]

def valid(row):
    try:
        obj = json.loads(row['output'])
        return row.get('finish') == 'stop' and isinstance(obj.get('polished'), str) and bool(obj['polished'].strip()) and isinstance(obj.get('corrections'), list) and isinstance(obj.get('key_terms'), list) and all(isinstance(c, dict) and isinstance(c.get('original'), str) and isinstance(c.get('corrected'), str) and c.get('type') in ['homophone', 'term', 'pronoun', 'style'] for c in obj['corrections']) and all(isinstance(t, str) for t in obj['key_terms'])
    except (ValueError, TypeError, AttributeError):
        return False

def main():
    groups = {}
    metrics = []
    for path in sorted((ROOT / 'build').glob('app-evaluation-*.jsonl')):
        rows = [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]
        assert len(rows) == 60
        key = rows[0]['model'].split('-Q8')[0] + ' / ' + rows[0]['backend'].upper()
        groups[key] = rows
        times = sorted(row['totalSeconds'] for row in rows)
        first = sorted(row['firstTokenSeconds'] for row in rows if row.get('firstTokenSeconds') is not None)
        metrics.append({'modelDevice': key, 'samples': len(rows), 'schemaValid': sum(valid(r) for r in rows), 'generationMedianMs': round(statistics.median(times) * 1000), 'generationP95Ms': round(times[math.ceil(.95 * len(times)) - 1] * 1000), 'firstTokenMedianMs': round(statistics.median(first) * 1000), 'firstTokenP95Ms': round(first[math.ceil(.95 * len(first)) - 1] * 1000)})
    assert len(groups) == 4
    target = ROOT / 'docs'
    target.mkdir(exist_ok=True)
    (target / 'local-ai-metrics.json').write_text(json.dumps(metrics, ensure_ascii=False, indent=2), encoding='utf-8')
    esc = html.escape
    body = ['<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>本地双模型评测</title><style>body{font:15px/1.6 system-ui;margin:32px;color:#203040}table{border-collapse:collapse;width:100%;margin:20px 0}th,td{border:1px solid #ccd4df;padding:10px;vertical-align:top}th{background:#eef3f9}pre{white-space:pre-wrap;font-size:12px}.bad{background:#fff0ef}.scroll{overflow:auto}button{padding:8px 14px;margin-right:8px}small{color:#617080}</style><h1>本地双模型：60 条固定样例</h1><p>开发机：i9-11950H、64GB RAM、RTX A4000 Laptop 8GB；Windows x64，llama.cpp b11118，Q8，8192 上下文，关闭思考，单并发。复用软件基础润色提示词和默认 off 结构策略，无用户术语、历史纠错或屏幕信息。</p><p><b>结构有效不等于语义正确。</b>这是一轮开发机测量，非隔离实验；时延仅为引擎请求，不包含识别、排队、完整请求分词校验及最终粘贴。云端对照、低配实机和完整流程验收尚未完成。人工审核状态：待审；下列提示是 AI 抽查。</p><h2>速度与结构</h2><table><tr><th>模型 / 设备</th><th>结构有效</th><th>首字中位数 / P95</th><th>生成中位数 / P95</th></tr>']
    for m in metrics:
        body.append(f'<tr><td>{esc(m["modelDevice"])}</td><td>{m["schemaValid"]}/60</td><td>{m["firstTokenMedianMs"]} / {m["firstTokenP95Ms"]} ms</td><td>{m["generationMedianMs"]} / {m["generationP95Ms"]} ms</td></tr>')
    body.append('</table><h2>已发现的质量问题</h2><ul><li>Qwen 中文 #12 遗漏“去苏州”；#4 遗漏“其他内容不要改”；#14 的断句可能改变模型切换的时间关系。</li><li>LFM 中文 #16 将具体要求改写成“整理后文本”；#4 遗漏限制；部分纠错明细生成了未支持的类型，会被软件拒绝。</li><li>两款模型都有术语字段过度提取的情况。这些结果不足以证明达到云端质量，不能用格式通过率表示准确率。</li></ul><h2>逐条比较</h2><p><button onclick="filterRows(\'all\')">全部</button><button onclick="filterRows(\'zh\')">中文</button><button onclick="filterRows(\'en\')">英文</button><button onclick="filterRows(\'mixed\')">混输</button></p><div class="scroll"><table><thead><tr><th>原文</th>')
    for key in groups:
        body.append(f'<th>{esc(key)}</th>')
    body.append('<th>云端对照</th></tr></thead><tbody>')
    for index, sample in enumerate(next(iter(groups.values()))):
        body.append(f'<tr data-language="{sample["language"]}"><td style="min-width:220px">{sample["language"]} #{sample["index"] + 1}<br>{esc(sample["original"])}</td>')
        for rows in groups.values():
            row = rows[index]
            try:
                polished = json.loads(row['output']).get('polished', row['output'])
            except ValueError:
                polished = row['output']
            body.append(f'<td class="{"" if valid(row) else "bad"}" style="min-width:230px">{esc(str(polished))}<br><small>{round(row["totalSeconds"] * 1000)} ms · {"结构有效" if valid(row) else "结构无效，应拒绝"}</small><details><summary>原始 JSON</summary><pre>{esc(row["output"])}</pre></details></td>')
        body.append('<td>未测</td></tr>')
    body.append('</tbody></table></div><script>function filterRows(lang){document.querySelectorAll("tr[data-language]").forEach(row=>row.hidden=lang!=="all"&&row.dataset.language!==lang)}</script></html>')
    (target / 'local-ai-evaluation.html').write_text(''.join(body), encoding='utf-8')
    print(json.dumps(metrics, ensure_ascii=True))

if __name__ == '__main__':
    main()

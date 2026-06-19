**English**  [中文](README.zh-CN.md)

<div align="center">

![BitFun](./png/BitFun_title.png)

</div>
<div align="center">

[![GitHub release](https://img.shields.io/github/v/release/GCWing/BitFun?style=flat-square&color=blue)](https://github.com/GCWing/BitFun/releases)
[![Website](https://img.shields.io/badge/Website-openbitfun.com-6f42c1?style=flat-square)](https://openbitfun.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow?style=flat-square)](https://github.com/GCWing/BitFun/blob/main/LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square)](https://github.com/GCWing/BitFun)

</div>

---

## Local AI Workbench Built Around the Code Agent

BitFun is a local AI workbench built around a Code Agent designed for long-horizon tasks, engineering execution, and token economy.

It can understand complex context, call tools, wait for results, correct deviations, and keep long-horizon tasks moving until they reach a deliverable state. Coding, research, office work, documents, desktop operations, and extensible workflows all happen in the same local desktop environment.

Core goal: move AI from iterative Agent Loop execution into a productivity system that can autonomously complete long-horizon work.

![readme_hero](./png/readme_hero_CN.png)

---

## Agent Core Metrics

The data below evaluates BitFun's core Agent capabilities. All measurements use **Deepseek-V4-Pro** and are grouped into completion results, token economy, and other experience metrics.

> The current numbers are BitFun's initial evaluation results, with each case run once. Benchmarks can fluctuate with task sampling, model versions, runtime environment, and single-run variance, so these scores are meant as an initial sanity signal that the current Agent is already reasonably capable, not as a fixed ranking claim or final ceiling. We will keep optimizing and release full benchmark details later.

### 1. Completion Results

BitFun leads Open Code and Claude Code on both **SWE-Bench-Pro** and **SWE-Bench-Verified**. SWE-Bench-Pro focuses on complex software engineering, while SWE-Bench-Verified focuses on human-verified GitHub issue fixes.

![Agent benchmark scores](./png/agent_benchmark_scores.svg)

Benchmark references: [SWE-Bench-Pro](https://labs.scale.com/leaderboard/swe_bench_pro_public) / [SWE-Bench-Verified](https://www.swebench.com/verified.html)

### 2. Token Economy

Agent economy needs to be evaluated across end-to-end token consumption, execution time, and KV Cache reuse. The current snapshot first covers KV Cache behavior from the same SWE-Bench-Pro round: BitFun's average KV Cache hit rate was **98.67%**. The follow-up full benchmark report will add the broader cost and latency metrics.

![KV Cache hit rate distribution](./png/kv_cache_hit_rate.svg)

### 3. Other Experience Metrics

Beyond cost, Agent experience also depends on how quickly it can retrieve context in very large engineering projects. For tens-of-millions-line repositories such as Chromium, BitFun uses **flashgrep** to reduce search time by up to about **94.6%**, with an average speedup of about **36.1x**.

![flashgrep search speed](./png/flashgrep_search_speed.svg)

---

## One Desktop, Five Agent Workflows

| Workflow | What it solves |
| --- | --- |
| **Code** | A Code Agent for real repositories: Agentic, Plan, Debug, testing, review, Deep Review, and continuous iteration. |
| **Research** | Collect context, compare sources, summarize findings, and produce structured conclusions, reports, or follow-up actions. |
| **Cowork** | Handle PDF / DOCX / XLSX / PPTX, writing, rewriting, summarization, layout, and office collaboration. |
| **Operate** | Use Computer Use to operate browsers and desktop apps, completing flows such as clicking, typing, navigating, waiting, and confirming. |
| **Extend** | Connect MCP, install Skills, define Markdown Agents, generate Mini Apps, and continue reshaping BitFun itself. |

![first_screen_screenshot](./png/first_screen_screenshot.png)

---

## Ready Out of the Box

### Download directly

Go to [Releases](https://github.com/GCWing/BitFun/releases) to download the latest desktop installer. After installation, configure your model and start using BitFun.

### Run from source

**Prerequisites:**

- [Node.js](https://nodejs.org/) (LTS recommended)
- [pnpm](https://pnpm.io/)
- [Rust toolchain](https://rustup.rs/)
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```bash
pnpm install
pnpm run desktop:dev
```

For more development details, see [CONTRIBUTING.md](./CONTRIBUTING.md).

---

## Customize Your BitFun

BitFun's extension paths progress continuously from light to deep customization:

| Tier | Path | Best for |
| --- | --- | --- |
| **L1** | Markdown Agent | Defining roles, flows, constraints, and tool bundles. |
| **L2** | MCP / Skills | Connecting external tools, professional capabilities, and workflows. |
| **L3** | Mini App | Generating dedicated interfaces, forms, panels, or visualizations for tasks. |
| **L4** | Source-level customization | Changing tools, adapters, UI, Runtime, or product shape. |

You can use BitFun's Code Agent to extend BitFun itself.

---

## Contributing

Stars, Issues, and PRs are welcome. We especially care about:

1. Code Agent, Deep Review, debugging, and long-task execution capabilities
2. Cowork, research, document, and desktop workflows
3. MCP, Skills, Mini App, LSP plugins, and new domain Agents
4. Runtime stability, performance, context efficiency, and verifiability

Please submit PRs directly to the `main` branch. For more details, see [CONTRIBUTING.md](./CONTRIBUTING.md).

---

## Disclaimer

1. This project is spare-time exploration and research into next-generation human-machine collaboration, not a commercial profit-making project.
2. This project is 97%+ built through Vibe Coding. Code feedback is welcome, and AI-assisted refactoring and optimization are encouraged.
3. This project depends on and references many open-source projects. Thanks to all open-source authors. **If your rights are affected, please contact us for remediation.**

---

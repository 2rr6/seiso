---
kind: plan
---

# 实际使用问题审计（2026-09-28）

本页记录一次面向"实际投入使用"的审计结论，供后续修复参照。每条问题修复后请删除对应条目；全部处理完毕后删除本文件及 [文档索引](../README.md) 中的链接。

审计对象：`main` 分支 `82889b9`（已发布版本 v0.1.0 = `cc93e11`）。

方法：通读 README / docs / 规则文档与 `src/` 核心实现；本地 `cargo build --release` 后，在一个模拟的"普通开源项目"（含 README、CHANGELOG、CONTRIBUTING、`.github/` 模板、`docs/`、`node_modules/`、docs 站点风格链接、中英混排文档）里实际跑 `init` / `check` / `hook` / `--fix` / `--stdin-filename` 等命令，对照文档逐条验证。`cargo test --locked` 全部通过。

---

## 一、高优先级：会直接影响实际使用价值或造成误导

### 1. README 承诺的"约定"默认几乎不检查

- 注册表里 27 条规则，只有 5 条 stable：`KND001`、`KND002`、`LNK001`、`SUP001`、`SUP002`（`src/rules/mod.rs`）。其余 22 条（STL / PTR / VOX / RAT / ORD / MIX / EVD / DUP / OWN / LNK002）全部 preview，默认不启用。
- README "The convention" 列了 6 条原则，然后说 "seiso checks documents against this convention"。实测默认 `seiso init && seiso check` 实际只检查：有没有声明 kind、本地链接是否存在、suppression 语法。"一个事实只有一个家"、"长期页面不记录易变值"、"不对提问者说话" 等核心卖点在默认模式下**一条都不检查**。
- 复现：README 里写 `Current version: 1.2.3.` 和 `As you requested, I have added the feature.`，默认 `seiso check` 通过（exit 0）；加 `--preview` 才报 STL004 / VOX001 / VOX003。
- README 只有一句 "Default checks use accepted stable rules; `--preview` adds selected preview rules"，没有说明 stable 只有 5 条。`docs/release-notes/0.1.0.md` 反而写清楚了。建议 README 直接列出默认启用的规则，并明确 preview 的状态。

### 2. preview 规则的自评证据显示：目前基本只有误报

- `docs/evaluation/m1-2026-09-28.md`：STL001 holdout 7 条全误报（0%），PTR001 4/1。
- `docs/evaluation/m3-2026-09-28.md`：9 条 M3 规则 holdout 精度全为 0% 或"无样本"；编辑历史回放 26 条 post-change 诊断**全部为误报**；标注由 "authoring agent" 完成，"No independent or human labeling is claimed"。
- 也就是说，按项目自己的证据，`--preview` 目前对用户几乎只产生噪音。文档（integrations.md 第 8-10 行）却把 `--preview` 当成普通可选项推荐给 hook / pre-commit / CI。
- 建议：在 README / checking.md 明确 preview 的实验性质和已知精度，不要在集成文档里鼓励开 preview 做 gate。

### 3. 新项目首次运行 = 一堆 `KND001`，且没有合适的 kind 可选

- 语料库自己的数据：1,953 篇真实文档中 1,734 篇 kind 未知（m1 记录）。实测模拟项目 `seiso init` 后 `.github/ISSUE_TEMPLATE/*.md`、`PULL_REQUEST_TEMPLATE.md`、`CONTRIBUTING.md`、`docs/api/index.md` 全部报 `KND001`。
- 8 个 kind 里没有适合 CONTRIBUTING / CODE_OF_CONDUCT / SECURITY / issue & PR 模板 / 许可说明的类型。用户只能硬套 `howto`，或在每个文件里写 `allow-file KND001`。roadmap 里 "Whether projects need custom kinds" 仍是 open question。
- `seiso init` 生成的配置没有 `exclude`，也不提示 `.github/**` 这类目录如何处理。
- 建议：`init` 给出 `.github/**`、`node_modules/**`、`.venv/**`、`vendor/**` 等常见排除项；考虑增加一个不受 kind 约束的通用 kind（如 `misc`/`other`），或把 `KND001` 改成只在文件匹配到 `[[kinds]]` 范围之外时才报。

### 4. `LNK001`（stable、默认开）对文档站点常见写法误报

- 纯物理路径解析（`src/paths.rs::local_link_target` + `fs::metadata`），不做 `.md` / `index.md` 补全。
- 实测：`[Setup](docs/setup)`（文件是 `docs/setup.md`）→ 报 LNK001；`/site/docs/setup` 以**仓库根**解析而不是站点根 → 报 LNK001。Docusaurus / VitePress / MkDocs 用户会大量遇到，而这条是默认 gate 规则。
- `DOCS/setup.md`（大小写错误）在 Windows 上通过、Linux CI 上会失败——结果跟平台相关，文档没提。
- 消息文案 `Relative link target "/site/docs/setup"` 对根相对路径说 "Relative" 不准确。
- 建议：至少在 LNK001 文档和 checking.md 明确"不做扩展名/index 补全"，或提供 `lint.lnk.resolve-extensions` 之类选项；文案改为 "Local link target"。

### 5. 语言检测按"拉丁字母 vs 汉字数量"判定，中日文混英文标识符时整句被当成英文

- `src/md/parser.rs::detect_language`：每个句子统计 Latin / Han / Kana 字符数，`han + kana > latin` 才算中/日文；inline code 内容也计入 Latin。
- 实测（`--preview`）：`目前版本是 v1.2.3，配置见 \`docs/reference/configuration.md\` 与 \`src/config/mod.rs\`。` 不报 STL001；`根据你的要求，我已经把 \`DupSettings.min_identifiers\` 改成 …` 不报 VOX001。加 `lang: zh` frontmatter 后全部命中。
- 中文技术文档里这种混排是常态，等于中文用户的规则召回大幅下降。
- `lang` 覆盖只在 convention.md "Facts and pointers" 一段顺带提到，checking.md / configuration.md 都没写；也没有按路径配置默认语言的方式（只有 `lint.languages` 过滤器）。
- 建议：检测时忽略 inline code；或提供 `[[kinds]]` 类似的 `lang` 路径映射；并在配置文档里写明 `lang` frontmatter。

### 6. `.seiso_cache/` 无自带 `.gitignore`、无清理策略

- `check` 和 `hook claude-code` 默认在 workspace 根写 `.seiso_cache/`（`src/cache/mod.rs`），目录里没有 `.gitignore`（ruff / mypy / pytest 都会写一个 `*`），也没有 `CACHEDIR.TAG`。实测 `git status` 直接显示 `?? .seiso_cache/`。`seiso init` 不动 `.gitignore`，checking.md 也没提醒忽略。
- 缓存按内容哈希寻址，每个条目包含完整源文本，且**从不删除**；key 里含 `CARGO_PKG_VERSION`，升级后旧条目全部变成孤儿。配合编辑器 hook 每次保存都写一条，长期使用会无限增长。
- 建议：创建目录时写入 `.gitignore`（内容 `*`）；增加简单的过期清理；`init` 时提示。

### 7. `--config PATH` 会把 workspace 根改成配置文件所在目录，可能静默通过

- configuration.md 有写 "The workspace root is the selected configuration's directory"，但后果没写清楚。实测 `seiso check --config ci/seiso.toml`：整个仓库**不检查任何文件**，只打印 "No rules enabled…" 并 **exit 0**；`--config ci/seiso.toml README.md` 报 "outside workspace"。
- 在 CI 里把配置放子目录是很常见的做法，这里会得到一个"绿色但什么都没检查"的 gate。
- 建议：`--config` 只选择配置文件、不改变根；或至少在没有任何 selected 文件时返回非 0。

### 8. "No rules enabled for the selected files" 一条消息覆盖了至少 4 种不同原因

以下情况全部输出同一句话且 exit 0：
- 显式路径被 `.gitignore` 忽略（`seiso check node_modules/foo/README.md`）
- 显式路径被 `exclude` 排除
- 显式路径不是 Markdown（`seiso check notes.txt`）
- `--select VOX001` 选了 preview 规则但没加 `--preview`（用户明确要求的规则被静默丢弃；ruff 在这种情况会警告 "has no effect because preview is not enabled"）
- `--config` 导致根目录变化（见第 7 条）

建议按原因分别提示，尤其是 preview 未开启和路径被忽略两种。

---

## 二、中优先级：逻辑/文案不一致或体验问题

### 配置与命令

1. **`seiso init` 只看 cwd**：`Workspace::discover` 能找到 git 根，但 `init` 把 `seiso.toml` 写到 `cwd`，并且只用 `cwd.join("README.md")` 判断是否存在（`src/commands.rs:445-502`）。在 `docs/` 下运行会生成 `docs/seiso.toml`，glob 全部以 `docs/` 为基准。README 说 "From the repository root" 但工具不做保护。父目录已有配置时 `init` 直接报错，无法为子目录建独立配置。
2. **`extend` 的合并语义未文档化**：`overlay()` 对 table 递归合并、对数组整体替换（`src/config/mod.rs:633-646`）。子配置写一条 `[[kinds]]` 会**覆盖**父配置全部 `kinds`；`lint.select`/`ignore` 同理。configuration.md 只说 "opts into inheritance"。配置层面也没有 `extend-select` / `extend-ignore`，只有 CLI `--extend-select`。
3. **默认 `include` 与 `init` 写出的不一致**：`Settings::default()` 是 `["**/*.md"]`，`init` 写的是 `["**/*.md", "**/*.markdown"]`，而 `is_markdown()` 两者都认。无配置时 `.markdown` 被发现但被 include 过滤掉，静默跳过。
4. **kind 大小写敏感**：`kind: HowTo` / `how-to` 报 KND002。合理，但文档没写；KND002 的建议文案可加 "lowercase"。
5. **frontmatter YAML 错误的行号是相对 frontmatter 内容的**：`title: Note: colon` 报 "…at line 1 column 12"，实际文件第 2 行；诊断定位仍是 1:1。对着文件找会找错行。

### 规则与诊断

6. **STL001 与 STL004 对同一位置双报**：`目前版本是 v1.2.3` 同一 span 同时报 STL001 和 STL004，两条建议内容近似。
7. **STL001 的英文 stale 词表不含 "current"**：只有 `currently` / `latest` / `at present`。README 首段举的例子 "a stale version number" 最常见写法 `Current version: 1.2.3` 靠 STL004 才能命中。
8. **EVD001 把 "we recommend" / "is recommended" 视为需证据的评价**：how-to 里 "We recommend X" 是正常写法；VOX003 的 narration 词表含 "this document was generated"——这恰恰是很多 README 用来提醒读者"别手改"的合法声明。（都是 preview，但词表设计值得复查。）
9. **`seiso rule`**：`seiso rule knd001` 报 "Rule … is not implemented"（其实是大小写问题，措辞误导）；输出顶部原样打印了 `---\nkind: reference\n---` frontmatter；`--all` 按注册表顺序而非按代码排序（STL002、STL004、RAT001…）。
10. **标点不一致**：`SUP002 Suppression for KND002 did not suppress any diagnostic Suggestion: …`（缺句号，`src/rules/suppression.rs:372`）；KND001 frontmatter 错误消息同样缺句号。其余消息都以句号结尾。
11. **JSON 输出 `url` 字段恒为 null**：规则文档在 GitHub 有稳定 URL，可以填。
12. **文本输出 "Found 5 errors."**：文档其它地方统一叫 diagnostics / violations，且 convention 明说没有 warning 层级，这里突然叫 errors。

### 规则文档（`docs/rules/*.md`）结构不一致

13. 标题："Why it matters" vs "Why this matters"（PTR001/PTR003/RAT002/STL001/STL003/VOX001 用后者）；EVD001/MIX001/ORD001/ORD002/RAT001/STL002/STL004/VOX002/VOX003 缺该节。
14. SUP001/SUP002 用 "## Content read"，其余用 "## Inputs"；SUP001/SUP002 标题小写（"invalid suppression"），其余首字母大写。
15. DUP002 有两个 "## Settings" 小节。

### 分发与集成

16. **npm 包对 macOS / arm64 直接安装失败**：`package.json` 声明 `"os": ["linux","win32"], "cpu": ["x64"]`，npm 会以 EBADPLATFORM 拒绝安装，**没有**源码回退。README 写 "Other platforms build from source" 对 npm 渠道不成立（pip 渠道确实会退到 sdist + maturin）。开发者工具缺 macOS 二进制是显著缺口；Alpine（musl）上 linux-x64 glibc 二进制也起不来，`bin/seiso.cjs` 的报错只会说 "cannot start the bundled binary"。
17. **pre-commit hook 需要 Rust 工具链**（`language: python` → maturin 源码构建），文档有写；但 `.pre-commit-hooks.yaml` 没有 `minimum_pre_commit_version`，integrations.md 用 `<reviewed-revision>` 占位而不直接给 `v0.1.0`。
18. **hook 也写缓存**：`hook claude-code` 用 `no_cache: false`，每次 Claude Code 编辑都会在用户仓库根写 `.seiso_cache/`（见第一部分第 6 条）。

### 版本与文档漂移

19. `main` 上有 9 条 v0.1.0 之后新增的规则（18 → 27）及 `sections.md`、`sections.rs`，`Cargo.toml` / `package.json` 版本仍是 `0.1.0`。用户按 GitHub main 文档 `seiso rule STL002` 会得到 "not implemented"。roadmap 已把 M3 写成 delivered。
20. `docs/guides/development.md` 和 CI 用 `cargo run -p seiso`，但这是单包项目，`-p seiso` 冗余；工作区里还残留空的 `crates/seiso_*` 目录（未跟踪，本地清理即可）。
21. `docs/reference/configuration.md` 说 "Rule documentation identifies each rule's thresholds and word-list options"，但只有部分规则文档真的写了阈值/词表；`lint.dup.*` 五个字段的默认值只能去读 `src/config/mod.rs`。

---

## 三、低优先级 / 措辞

1. README 开篇 "Hardly anyone writes project docs by hand anymore. AI writes most of them" 是无依据的断言——项目自己的 EVD001 就是针对这类 "evaluative claims without evidence"。可以改成更克制的表述。
2. README 用 rustfmt 类比 "without agreeing on them first"，但 seiso 实际要求每个项目先配置 kind 映射、给每篇文档定 kind，配置负担和 rustfmt 的零配置正相反。类比容易让人预期落空。
3. README 说 "Each diagnostic says where the problem is and how to fix it, so an agent can repair the page from seiso's output alone"——对 KND/LNK/SUP 成立；对 DUP/OWN 的建议是 "choose one authoritative document"，agent 无法仅凭输出决定。
4. integrations.md Claude Code 一节的 matcher `"Write|Edit"` 是正则，会同时匹配 `MultiEdit`（对 seiso 无害，但读者可能以为只匹配两个工具）。
5. `seiso check --help` 里 `--stdin-filename` 的描述 "never writes to disk" 与 `--fix` 互斥关系只能靠 clap 冲突报错得知，help 文本未说明。
6. Text 渲染里每条诊断都带源码摘录，一个 300 篇文档的仓库首次 `check` 会输出上千行 KND001；concise 更适合作为默认，或至少在 KND001 大量出现时折叠。

---

## 四、核验为正确/做得好的部分（避免只报问题）

- 文档对实现的描述总体准确：配置发现顺序、`last wins`、`--select` 替换 / `--extend-select` 追加、exit code 0/1/2、`--exit-zero` 保留 2、stdin 覆盖、`--fix` 的二次校验与写前源比对、hook exit 0/1/2 映射、`policy` 的 `not_evaluated` 语义，均与实测一致。
- CRLF、UTF-8 BOM、Windows 反斜杠路径、从子目录运行、路径不存在、非法 selector 等边界情况处理正确，错误信息清楚。
- `--fix` 只删确认无效的 suppression code、保留原因和其它 code、保持 CRLF；写入前比对源文件；拒绝 symlink / 只读文件。
- 评估记录（`docs/evaluation/*`）对自身局限（agent 标注、样本为零、精度不可用）的陈述非常诚实；release notes 对 stable/preview 边界的说明清楚。
- CI 结构合理（docs-only 分流、Linux+Windows 测试、`check` 汇总 job）；`cargo test --locked` 本地全部通过。
- 输出格式（text/concise/json/sarif/github）齐全，GitHub 输出对 `##[` 注入做了防护。

---

## 五、建议的处理顺序

1. README 明确列出默认启用的 5 条规则和 preview 的实验状态（第一部分 1、2）。
2. `.seiso_cache/` 自带 `.gitignore` + `init` 生成常见 `exclude`（第一部分 3、6）。
3. 区分 "No rules enabled" 的各类原因，preview 规则被选中但未开启时给出明确提示（第一部分 8）。
4. `--config` 不改变 workspace 根，或无 selected 文件时非 0 退出（第一部分 7）。
5. 语言检测忽略 inline code，并在配置文档写明 `lang` frontmatter（第一部分 5）。
6. LNK001 文档写明不做扩展名/index 补全，评估是否提供选项（第一部分 4）。
7. 增补 macOS 二进制或在 README 修正 npm 渠道的表述（第二部分 16）。
8. 统一规则文档结构与诊断标点（第二部分 10、13-15）。

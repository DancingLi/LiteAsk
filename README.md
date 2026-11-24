# LiteAsk

轻量级跨平台（**macOS & Windows**）划词翻译 / 解释 & 全局快捷键桌面 AI 弹窗助手，基于 **Tauri v2 + Rust 原生划词 Hook** 构建，告别 Electron 的大体积与高内存占用（安装包仅 ~3.8 MB）。

## 核心功能

1. **划词悬浮胶囊（鼠标松开自动弹出 / 一键随时开关）**
   - 拖选文字或双击/三击选词松开后，在光标附近自动弹出不抢焦点的轻量胶囊条：**翻译** | **解释** | **问 AI** | **复制**。
   - 偏好设置页顶部与系统托盘菜单均支持**一键即时开启/关闭划词悬浮条显示**。
   - **双通道原生取词（借鉴 CherryStudio 方案）**：
     - **macOS**：`CGEventTap` 监听 + `AXUIElement` 辅助功能优先零剪贴板取词 + 剪贴板完整备份/还原兜底（完美支持 Chrome / Electron / PDF）。
     - **Windows**：`WH_MOUSE_LL` 监听 + `IUIAutomation TextPattern` 优先取词 + `Ctrl+C` 剪贴板备份/还原兜底 + `WS_EX_NOACTIVATE` 不抢焦点浮窗。
2. **自定义模型供应商 & 任意模型 ID**
   - 内置 **OpenCode Go**（默认 `deepseek-v4.1-flash`，支持一键从本机 `~/.local/share/opencode/auth.json` 导入 Key）。
   - 支持**添加任意自定义 OpenAI 兼容供应商**（自定义名称、Base URL、API Key、模型 ID），内置 DeepSeek 官方、OpenAI、OpenRouter、硅基流动、本地 Ollama 快捷模板，支持实时拉取接口模型列表与一键连通性测试。
3. **按场景自定义模型思考强度（Reasoning Effort）**
   - **划词翻译**：默认关闭思考（`none`），直出译文、极速响应。
   - **划词解释** & **桌面弹窗助手**：可在设置页按场景自由配置思考强度（`关闭 / 极简 / 低 / 中 / 高 / 最高 / 默认`），浮窗顶部也支持随时临时切换。
4. **全局快捷键桌面弹窗助手（豆包 / Spotlight 风格）**
   - 默认快捷键 `Option+Space`（macOS）/ `Alt+Space`（Windows），可在设置中自定义录制，支持多轮追问与 Markdown 渲染。
5. **内置 GitHub Releases 自动更新**
   - 基于 Tauri v2 Updater 签名校验机制，启动自动静默检查更新，也可在设置页或托盘菜单点击「检查更新」一键下载并重启升级。

## 本地开发与发版

```bash
npm install
npm run tauri:dev      # 本地开发调试
npm run tauri:build    # 打包正式版
```

### 发布新版本（自动触发 macOS + Windows 双端构建与热更新）
1. 同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的 `version`。
2. 推送版本 Tag：
   ```bash
   git tag v0.2.1 && git push origin v0.2.1
   ```
3. GitHub Actions 将自动编译、签名并发布 macOS（Apple Silicon + Intel）和 Windows 安装包及 `latest.json`，所有已安装客户端即可一键自动更新。

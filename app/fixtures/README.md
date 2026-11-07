# fixtures

CLI 输出样本目前**内联**在代码里，不放在这个目录：

- `src-tauri/crates/ocw-device/src/sim.rs` —— 可运行的样本（读卡/空白/管理卡/无卡/自检/端口）
- `src-tauri/crates/ocw-device/src/parse.rs` 的 `#[cfg(test)] mod tests` —— 解析器的边界样本

原因：样本是**契约**，跟着解析器一起改、一起跑；放在外部文件反而容易与代码脱节。
等样本数量上来（例如接入更多读卡器型号）再拆成 `fixtures/*.json` + `include_str!`。

// 发布版不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod backup_job;
mod cardno;
mod commands;
mod diagnostics;
mod logging;
#[cfg(debug_assertions)]
mod demo_import;
mod provision;
mod room_import;
mod rules;
mod state;

use std::sync::{Arc, Mutex};

use tauri::Manager;

use crate::state::{build_device, AppState, DeviceSlot};

fn main() {
    // 把 panic 明确打到 stderr：dev 模式下终端/日志里能直接读到，不用去猜窗口为什么白屏
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[rust][PANIC] {info}");
        crate::logging::line("ERROR", "panic", &format!("{info}"));
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二个实例只把已有窗口叫到前台：串口是独占资源，不能有两个发卡进程。
            // 窗口可能被缩小到托盘（hide），所以要先 show 再 unminimize/focus。
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let root = provision::data_root();
            std::fs::create_dir_all(&root)?;

            let db_path = root.join("opencardwright.db");
            let conn = ocw_store::open(&db_path)?;

            // 调试便利：空库时把仓库里的 NacreHis.json（Kf_fh 客房房号表）导进来，
            // 省得每次重置数据库都要手搓房号。发布版不编译这段。
            #[cfg(debug_assertions)]
            {
                let rooms_json = std::path::Path::new(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../data/NacreHis.json"
                ));
                let empty = ocw_store::list_rooms(&conn)
                    .map(|r| r.is_empty())
                    .unwrap_or(false);
                if empty && rooms_json.exists() {
                    match demo_import::import_nacre_rooms(&conn, rooms_json) {
                        Ok(n) => eprintln!("[dev] 已从 NacreHis.json 导入 {n} 个房间（Kf_fh）"),
                        Err(e) => eprintln!("[dev] 房间导入失败：{e}"),
                    }
                }
            }

            // 文件日志：本地时区偏移问一次 SQLite（不引 chrono），失败也不影响启动
            let offset: i64 = conn
                .query_row(
                    "SELECT CAST(strftime('%s','now','localtime') AS INTEGER) - CAST(strftime('%s','now') AS INTEGER)",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            logging::init(root.join("logs"), offset);
            logging::line(
                "INFO",
                "start",
                &format!(
                    "启动 v{} · db={} · runtime={}",
                    env!("CARGO_PKG_VERSION"),
                    db_path.display(),
                    root.join("runtime").display()
                ),
            );

            let port: u8 = ocw_store::get_setting(&conn, "port")?
                .and_then(|v| v.parse().ok())
                .unwrap_or(6);

            let runtime = provision::ensure_runtime(app.handle());

            let device = build_device(&runtime.dir, port);
            if device.is_sim() {
                eprintln!("[ocw] 使用模拟设备（未找到 SDK，或设置了 OCW_SIM）");
            }

            let db = Arc::new(Mutex::new(conn));
            let db_for_job = db.clone();
            let db_path_for_job = db_path.clone();

            app.manage(AppState {
                db: db.clone(),
                db_path,
                slot: Arc::new(Mutex::new(DeviceSlot { device, port })),
                runtime_dir: runtime.dir,
                driver_sha256: runtime.driver_sha256,
                sdk_source: runtime.source,
                dev: runtime.dev,
                session: Arc::new(Mutex::new(None)),
            });

            // 自动备份：每 30 秒醒一次读设置，到点就备份（间隔 0 = 关闭）
            backup_job::spawn(db_for_job, db_path_for_job);

            // 关窗拦截（TD-109）：点 X / Alt+F4 不直接退，先问「缩小到托盘还是退出」，
            // 由前端弹确认框（默认缩小）；选择缩小走 hide_to_tray，退出走 quit_app。
            if let Some(win) = app.get_webview_window("main") {
                let w = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        use tauri::Emitter;
                        let _ = w.emit("close-confirm", ());
                    }
                });
            }

            // 系统托盘：缩小到托盘后靠它恢复 / 退出（TD-109）
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&show, &quit])?;
                let mut builder = TrayIconBuilder::with_id("main-tray")
                    .tooltip("门锁发卡台")
                    .menu(&tray_menu)
                    // 左键走 on_tray_icon_event（单击恢复窗口），菜单由右键弹出
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    builder = builder.icon(icon.clone());
                }
                builder.build(app)?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::auth_status,
            commands::initialize_admin,
            commands::login,
            commands::logout,
            commands::read_card,
            commands::list_rooms,
            commands::create_room,
            commands::rename_room,
            commands::delete_room,
            commands::issue_install_card,
            commands::issue_guest,
            commands::issue_admin_card,
            commands::list_departments,
            commands::create_department,
            commands::rename_department,
            commands::set_department_active,
            commands::list_room_types,
            commands::create_room_type,
            commands::rename_room_type,
            commands::delete_room_type,
            commands::set_room_type,
            commands::set_room_type_color,
            commands::list_room_statuses,
            commands::update_room_status_style,
            commands::set_room_status,
            commands::set_room_max_cards,
            commands::get_max_cards_default,
            commands::set_max_cards_default,
            commands::list_reservations,
            commands::list_reservation_records,
            commands::create_reservation,
            commands::update_reservation,
            commands::cancel_reservation,
            commands::checkout_room,
            commands::checkout_card,
            commands::force_checkout_card,
            commands::room_move_preview,
            commands::room_move_out,
            commands::backup_database,
            commands::list_backups,
            commands::restore_database,
            commands::get_backup_settings,
            commands::set_backup_settings,
            commands::import_backup_file,
            commands::export_diagnostics,
            commands::list_issuances,
            commands::list_audits,
            commands::export_issuances_csv,
            commands::import_rooms,
            commands::get_onboarding_status,
            commands::complete_onboarding,
            commands::get_checkout_status,
            commands::set_checkout_status,
            commands::log_frontend,
            commands::list_operators,
            commands::create_operator,
            commands::reset_operator_password,
            commands::set_operator_active,
            commands::change_own_password,
            commands::get_reminder_settings,
            commands::set_reminder_settings,
            commands::get_checkout_hour,
            commands::set_checkout_hour,
            commands::expiring_cards,
            commands::hide_to_tray,
            commands::quit_app,
            commands::run_selftest,
            commands::probe_ports,
            commands::set_port,
        ])
        .run(tauri::generate_context!())
        .expect("门锁发卡台启动失败");
}

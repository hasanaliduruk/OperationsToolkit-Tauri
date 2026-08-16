#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod converter;
mod cost_updater;
mod future_price_updater;
mod invoice_processor;
mod restock_processor;
mod tsv_converter;
mod order_creator;
mod shipment_creator;

use keyring::Entry;
use rfd::FileDialog;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::Mutex;

struct AppState {
    job_lock: Mutex<()>,
    cancel_flag: Arc<AtomicBool>,
}

fn get_settings_dir() -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push("Settings");
    if !path.exists() {
        fs::create_dir_all(&path).expect("Kritik Hata: Settings klasörü oluşturulamadı.");
    }
    path
}

fn ensure_default_settings() {
    let dir = get_settings_dir();
    let defaults: &[(&str, &str)] = &[
        ("costupdater_settings.json", r#"{"columns": {"cost": ["cost"], "sku": ["sku"], "additional cost": ["additional_cost"], "business pricing": ["business_pricing"], "bp strategy": ["bp_strategy"], "qd strategy": ["qd_strategy"]}, "warehouses": {"BX": 0.75, "CANDY": 0.75, "COS": 0.75, "CS": 0.75, "CSC": 0.75, "DL": 0.75, "FC": 0.75, "FD": 0.75, "FL": 0.75, "FOUR": 0.75, "FR": 0.75, "GEMCO": 0.75, "IL": 0.75, "JC": 0.75, "KH": 0.75, "LR": 0.75, "MD": 0.75, "MONIN PUMP SL": 0.75, "NC": 0.75, "NF": 0.75, "NJ": 0.75, "NK": 0.75, "NT": 0.75, "SN": 0.75, "UC": 0.75, "UD": 0.75, "UN": 0.75, "UPC": 0.75, "WB": 0.75, "WEBS": 0.75, "TD": 0.75, "IN": 0.75, "BL": 0.75, "YT": 0.75}}"#),
        ("costupdater2_settings.json", r#"{"columns": {"cost": ["cost"], "sku": ["sku"], "additional cost": ["additional_cost"], "business pricing": ["business_pricing"], "bp strategy": ["bp_strategy"], "qd strategy": ["qd_strategy"], "pkg volume": ["pkg_volume"], "pkg weight": ["pkg_weight"]}, "warehouses": {"BX": {"v2_additional_cost": 0.0, "v2_equation": 2, "v2_warehouse_fee": 0.70}, "CANDY": {"v2_additional_cost": 0.0, "v2_equation": 2, "v2_warehouse_fee": 0.70}, "COS": {"v2_additional_cost": 0.0, "v2_equation": 2, "v2_warehouse_fee": 0.70}, "FL": {"v2_additional_cost": 0.0, "v2_equation": 1, "v2_warehouse_fee": 0.70}, "IL": {"v2_additional_cost": 0.0, "v2_equation": 1, "v2_warehouse_fee": 0.70}, "TX": {"v2_additional_cost": 0.0, "v2_equation": 2, "v2_warehouse_fee": 0.70}}}"#),
        ("restock_settings.json", r#"{"columns": {"upc": ["UPC", "upc", "Upc", "UPC #"], "brand": ["BRAND", "Brand", "brand"], "price": ["NET_AMOUNT", "Price", "price"], "case": ["CASEPACK", "Size", "Case", "case", "size", "Case Pack"], "Quantity on hand": ["Qty on Hand", "Quantity Available"], "pk": ["PK"]}, "deposits": {"41 cost": 0.70, "41 standart": 0.70, "45 cost": 0.70, "45 standart": 0.70, "19 cost": 0.70, "19 standart": 0.70, "27 cost": 1.00, "27 standart": 1.00, "18 cost": 1.00, "18 standart": 1.00, "01 cost": 1.00, "01 standart": 1.00, "16 standart": 1.00, "NF": 0.70, "TD": 0.70, "BZ": 1.00, "YT": 1.00, "MI": 0.70, "PF": 0.70, "HN": 0.70, "BC": 1.00, "NW": 0.70, "TH": 0.70, "ST": 1.00, "FD": 0.70, "UN": 0.70, "PH": 0.70, "EJ": 0.70}}"#),
        ("ordercreate_settings.json", r#"{"restock_columns": { "upc": ["Upc"], "pcs": ["PCS"], "suplier": ["suplier"], "notes": ["Notes"] }, "orderform_columns": { "upc": ["UPC"], "pcs": ["PCS(TOTAL)"], "suplier": ["suplier"] }}"#),
        ("invoice_settings.json", r#"{"columns": { "remove": ["Status", "QuantityNotShipped", "InvalidReason"], "shipquantity": ["ShipQuantity"], "date": ["InvoiceDate"] }}"#),
        ("shipment_settings.json", r#"{"restock_columns": { "upc": ["Upc"], "pcs": ["PCS", "Pcs", "pcs"], "asin": ["ASIN"], "pk": ["PK"], "price": ["Price"], "suplier": ["suplier"] }, "orderform_columns": { "upc": ["UPC"], "pcs": ["PCS"], "asin": ["ASIN 1", "ASIN 2", "ASIN 3", "ASIN 4"], "sku": ["ASIN1_SKU", "ASIN2_SKU", "ASIN3_SKU", "ASIN4_SKU"], "pk": ["PK"], "price": ["price"], "suplier": ["suplier"] }, "invoice_columns": { "shipquantity": ["ShipQuantity"], "upc": ["Upc"], "price": ["NetEach2"], "packsize": ["PackSize"], "brand": ["Brand"], "description": ["Description"] }}"#),
        ("invoicefinder_yonergeler.txt", "Invoice Finder Programı Yönergeleri:\n\n1. Ekranınızda gözükmekte olan ilk boşluğa orada da belirtildiği üzere uygulamanın bulmuş olduğu invoice dosyalarının ve en son uygulamanın oluşturacağı excel dosyasının kaydedileceği dosya yolunu gerek elinizle yazarak gerek Browse butonunu kullanarak uygulamaya belirtiniz.\n\n2. İkinci boşluğa ise bilgisayarınızda bulunan bütün invoice pdf dosyalarını içeren klasörün yolunu 1. yönergede belirtildiği şekilde giriniz.\n\n3. Üçüncü boşluğa ise içeriğinde bütün Upc değerleri ve o değerlere karşılık gelen invoice numaralarını içeren ALL INVOICES excelini önceki maddelerde belirtildiği şekilde giriniz.\n\n4. Dördüncü boşluğa ise uygulamanın hangi tarihten önceki invoiceleri tarayacağını giriniz.\n\n5. İlk 3 Dosya yolunu \"Kaydet\" butonu arayıcılığıyla daha sonraki işlemlerinizde de kullanmak amacıyla kaydedebilirsiniz.\n\n6. Bütün bu dosya yolu girme yerlerinin altında bir adet sürükle ve bırak yöntemi ile dosya algılayan alan göreceksiniz. O alana Amazonun sitesinden kopyalayarak aldığınız verileri bir excele metin olarak yapıştırıp oluşturduğunuz excel dosyasını belirtilen alana fare imlecinizle tutup bırakınız.\n\n7. İşlemi başlatmak için \"Başlat\" butonunuza basmanız yeterlidir.\n\n-------------SÜTUN İSİMLERİ---------------\n\nALL INVOICES EXCEL DOSYASI İÇİN:\n\nship quantity = ShipQuantity\nitem number = ShipItem\nUPC = Upc\nInvoice Number = InvoiceNumber\nDate = Date")
    ];
    for (filename, content) in defaults {
        let mut file_path = dir.clone();
        file_path.push(filename);
        if !file_path.exists() {
            let _ = fs::write(file_path, content);
        }
    }
    let mut template_dir = dir.clone();
    template_dir.push("Template");
    if !template_dir.exists() {
        let _ = fs::create_dir_all(template_dir);
    }
}

#[tauri::command]
fn get_settings(file_name: String) -> Result<String, String> {
    let mut path = get_settings_dir();
    path.push(&file_name);
    if !path.exists() { return Ok("{}".to_string()); }
    fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(file_name: String, settings: String) -> Result<(), String> {
    let mut path = get_settings_dir();
    path.push(&file_name);
    fs::write(path, settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_memory() -> Result<Value, String> {
    let mut path = get_settings_dir();
    path.push("last_paths.json");
    if !path.exists() { return Ok(serde_json::json!({})); }
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_memory_value(key: String, value: Value) -> Result<(), String> {
    let mut path = get_settings_dir();
    path.push("last_paths.json");
    let mut memory: Value = if path.exists() {
        let data = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if let Some(obj) = memory.as_object_mut() {
        obj.insert(key, value);
    }
    let out = serde_json::to_string_pretty(&memory).map_err(|e| e.to_string())?;
    fs::write(path, out).map_err(|e| e.to_string())
}

#[tauri::command]
fn pick_folder() -> Result<Option<String>, String> {
    if let Some(folder) = FileDialog::new().pick_folder() {
        Ok(Some(folder.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn pick_files(multiple: bool) -> Result<Vec<String>, String> {
    let dialog = FileDialog::new();
    if multiple {
        if let Some(files) = dialog.pick_files() {
            Ok(files.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
        } else {
            Ok(vec![])
        }
    } else {
        if let Some(file) = dialog.pick_file() {
            Ok(vec![file.to_string_lossy().to_string()])
        } else {
            Ok(vec![])
        }
    }
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(&path).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_settings_folder() -> Result<(), String> {
    let path = get_settings_dir();
    open_folder(path.to_string_lossy().to_string())
}

#[tauri::command]
fn open_template_folder() -> Result<(), String> {
    let mut path = get_settings_dir();
    path.push("Template");
    if !path.exists() { fs::create_dir_all(&path).map_err(|e| e.to_string())?; }
    open_folder(path.to_string_lossy().to_string())
}

#[tauri::command]
fn get_expiration_credentials() -> Result<Value, String> {
    let memory_str = get_memory()?;
    let username = memory_str.get("expiration_username").and_then(|v| v.as_str()).unwrap_or("");
    let mut password = String::new();
    if !username.is_empty() {
        if let Ok(entry) = Entry::new("OperationsToolkit-2DWorkflow", username) {
            password = entry.get_password().unwrap_or_default();
        }
    }
    Ok(serde_json::json!({ "username": username, "password": password }))
}

#[tauri::command]
fn save_expiration_credentials(username: String, password: String) -> Result<(), String> {
    set_memory_value("expiration_username".to_string(), serde_json::Value::String(username.clone()))?;
    if !username.is_empty() {
        if let Ok(entry) = Entry::new("OperationsToolkit-2DWorkflow", &username) {
            let _ = entry.set_password(&password);
        }
    }
    Ok(())
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn run_costupdater(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file: String,
    output_folder: String,
    settings: Value,
    version: u8,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    app.emit("job-log", serde_json::json!({ "message": format!("Dosya işleniyor (V{})...", version), "color": "white" })).map_err(|e| e.to_string())?;

    match cost_updater::process(&app, &state.cancel_flag, &file, &output_folder, settings, version) {
        Ok(output_path) => {
            let msg = format!("V{} İşlemi başarıyla tamamlandı!", version);
            app.emit("job-done", serde_json::json!({ "ok": true, "message": msg, "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_invoice(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    files: Vec<String>,
    output_folder: String,
    settings: Value,
    del_zero: bool,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    app.emit("job-log", serde_json::json!({ "message": "Faturalar O(1) bellek modeliyle Excel'e akıtılıyor...", "color": "white" })).map_err(|e| e.to_string())?;

    match invoice_processor::process(&app, &state.cancel_flag, files, output_folder, settings, del_zero) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": "İşlem vektörel hızda başarıyla tamamlandı!", "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_tsv(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    files: Vec<String>,
    output_folder: String,
    save_name: String,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    app.emit("job-log", serde_json::json!({ "message": "TSV verileri bellekte birleştiriliyor...", "color": "white" })).map_err(|e| e.to_string())?;

    match tsv_converter::process(&app, &state.cancel_flag, files, output_folder) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": format!("Veriler başarıyla {} dosyasına yazıldı.", save_name), "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_converter(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    files: Vec<String>,
    output_folder: String,
    input_type: String,
    output_type: String,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    app.emit("job-log", serde_json::json!({ "message": "Dosyalar dönüştürülüyor...", "color": "white" })).map_err(|e| e.to_string())?;

    match converter::process(&app, &state.cancel_flag, files.clone(), output_folder, input_type, output_type) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": format!("{} dosya başarıyla dönüştürüldü!", files.len()), "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_future_price(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    restock_file: String,
    future_file: String,
    save_name: String,
    output_folder: String,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    app.emit("job-log", serde_json::json!({ "message": "Vektörel eşleştirme yapılıyor...", "color": "white" })).map_err(|e| e.to_string())?;

    match future_price_updater::process(&app, &state.cancel_flag, output_folder, save_name, restock_file, future_file) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": "Future Price işlemi başarıyla tamamlandı!", "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_restock(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ham_files: Vec<String>,
    export_files: Vec<String>,
    restock_files: Vec<String>,
    do_export: bool,
    do_restock: bool,
    save_name: String,
    output_folder: String,
    settings: Value,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    
    match restock_processor::process(&app, &state.cancel_flag, ham_files, export_files, restock_files, do_export, do_restock, save_name, output_folder, settings) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": "Restock işlemi başarıyla tamamlandı!", "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_order_creator(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    restock_files: Vec<String>,
    orderform_files: Vec<String>,
    template_path: String,
    output_folder: String,
    settings: Value,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    
    match order_creator::process(&app, &state.cancel_flag, restock_files, orderform_files, template_path, output_folder, settings) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": "Order Create işlemi başarıyla tamamlandı!", "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
async fn run_shipment_creator(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    invoice_files: Vec<String>,
    order_form_files: Vec<String>,
    restock_files: Vec<String>,
    output_folder: String,
    save_name: String,
    dc_code: String,
    settings: Value,
) -> Result<Value, String> {
    let _lock = state.job_lock.try_lock().map_err(|_| {
        app.emit("job-log", serde_json::json!({ "message": "Sistemde zaten çalışan bir işlem var.", "color": "red" })).unwrap();
        "Sistemde zaten çalışan bir işlem var.".to_string()
    })?;

    state.cancel_flag.store(false, Ordering::Relaxed);
    
    match shipment_creator::process(&app, &state.cancel_flag, invoice_files, order_form_files, restock_files, output_folder, save_name, dc_code, settings) {
        Ok(output_path) => {
            app.emit("job-done", serde_json::json!({ "ok": true, "message": "Shipment Create işlemi başarıyla tamamlandı!", "output_path": output_path })).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            app.emit("job-done", serde_json::json!({ "ok": false, "message": e })).map_err(|err| err.to_string())?;
            Err(e)
        }
    }
}

fn main() {
    ensure_default_settings();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            job_lock: Mutex::new(()),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_memory,
            set_memory_value,
            pick_folder,
            pick_files,
            open_folder,
            open_settings_folder,
            open_template_folder,
            get_expiration_credentials,
            save_expiration_credentials,
            cancel_job,
            run_costupdater,
            run_invoice,
            run_tsv,
            run_converter,
            run_future_price,
            run_restock,
            run_order_creator,
            run_shipment_creator
        ])
        .run(tauri::generate_context!())
        .expect("Kritik Hata: Tauri motoru başlatılamadı.");
}
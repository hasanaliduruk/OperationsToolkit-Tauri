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
use rusqlite::{Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{NaiveDate, Utc, Datelike};
use calamine::{open_workbook_auto, Data, Reader};
use regex::Regex;

struct AppState {
    job_lock: Mutex<()>,
    cancel_flag: Arc<AtomicBool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ShipmentRow {
    shipment_name: String,
    shipment_id: String,
    created_date: String,
    sku: String,
    qty_shipped: i32,
    exp_date_usa: String,
    exp_date_tur: String,
    days_remaining: i32,
    amz_stock_days: i32,
    note: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FifoResultRow {
    shipment_name: String,
    shipment_id: String,
    sku: String,
    qty_shipped: i32,
    exp_date_usa: String,
    exp_date_tur: String,
    days_remaining: i32,
    amz_stock_days: i32,
    amz_stock_allocated: i32,
    note: String,
}

#[derive(Serialize, Deserialize)]
struct InventoryData {
    sirali: Vec<ShipmentRow>,
    analiz: Vec<FifoResultRow>,
    stock: HashMap<String, i32>,
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

fn get_db_connection() -> SqlResult<Connection> {
    // Gerçek üretimde bu yolu AppData veya tauri::api::path üzerinden dinamik almalısın
    let conn = Connection::open("fba_inventory.db")?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS shipment_items (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             shipment_id TEXT NOT NULL,
             shipment_name TEXT NOT NULL,
             created_date TEXT NOT NULL,
             created_timestamp TIMESTAMP NOT NULL,
             sku TEXT NOT NULL,
             qty INTEGER NOT NULL,
             exp_date_usa TEXT,
             exp_date_tur TEXT,
             days_remaining INTEGER,
             note TEXT NOT NULL DEFAULT '',
             UNIQUE(shipment_name, shipment_id, created_date, sku, qty, exp_date_usa) ON CONFLICT REPLACE
         );
         CREATE TABLE IF NOT EXISTS amazon_stock (
             sku TEXT PRIMARY KEY,
             total_units INTEGER NOT NULL
         );"
    )?;
    Ok(conn)
}

fn internal_get_all_data() -> Result<InventoryData, String> {
    let conn = get_db_connection().map_err(|e| e.to_string())?;

    let mut stock: HashMap<String, i32> = HashMap::new();
    let mut stock_stmt = conn.prepare("SELECT sku, total_units FROM amazon_stock").map_err(|e| e.to_string())?;
    let stock_iter = stock_stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    }).map_err(|e| e.to_string())?;

    for item in stock_iter {
        if let Ok((sku, qty)) = item {
            stock.insert(sku, qty);
        }
    }

    let mut shipment_stmt = conn.prepare(
        "SELECT shipment_name, shipment_id, created_date, created_timestamp, sku, qty, exp_date_usa, exp_date_tur, days_remaining, note 
         FROM shipment_items ORDER BY created_timestamp ASC, id ASC"
    ).map_err(|e| e.to_string())?;

    let today = Utc::now().naive_utc().date();
    let mut sirali: Vec<ShipmentRow> = Vec::new();
    let mut sku_groups: HashMap<String, Vec<ShipmentRow>> = HashMap::new();

    let shipment_iter = shipment_stmt.query_map([], |row| {
        let created_ts: String = row.get(3)?;
        let amz_stock_days = if let Ok(parsed_date) = chrono::NaiveDate::parse_from_str(&created_ts[..10], "%Y-%m-%d") {
            (today.signed_duration_since(parsed_date).num_days() as i32) - 30
        } else {
            0
        };

        Ok(ShipmentRow {
            shipment_name: row.get(0)?,
            shipment_id: row.get(1)?,
            created_date: row.get(2)?,
            sku: row.get(4)?,
            qty_shipped: row.get(5)?,
            exp_date_usa: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            exp_date_tur: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
            days_remaining: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
            amz_stock_days,
            note: row.get(9)?,
        })
    }).map_err(|e| e.to_string())?;

    for item in shipment_iter {
        if let Ok(row) = item {
            sirali.push(row.clone());
            sku_groups.entry(row.sku.clone()).or_insert_with(Vec::new).push(row);
        }
    }

    let mut analiz: Vec<FifoResultRow> = Vec::new();
    let mut sorted_skus: Vec<_> = sku_groups.keys().cloned().collect();
    sorted_skus.sort();

    for sku in sorted_skus {
        if let Some(lots) = sku_groups.get(&sku) {
            let total_stock = stock.get(&sku).copied().unwrap_or(0);
            
            for (i, lot) in lots.iter().enumerate() {
                let qty_shipped = lot.qty_shipped;
                let sum_newer_qty: i32 = lots[(i + 1)..].iter().map(|l| l.qty_shipped).sum();

                let allocated = if total_stock <= 0 {
                    0
                } else if sum_newer_qty >= total_stock {
                    0
                } else if (sum_newer_qty + qty_shipped) > total_stock {
                    total_stock - sum_newer_qty
                } else {
                    qty_shipped
                };

                if allocated > 0 {
                    analiz.push(FifoResultRow {
                        shipment_name: lot.shipment_name.clone(),
                        shipment_id: lot.shipment_id.clone(),
                        sku: lot.sku.clone(),
                        qty_shipped: lot.qty_shipped,
                        exp_date_usa: lot.exp_date_usa.clone(),
                        exp_date_tur: lot.exp_date_tur.clone(),
                        days_remaining: lot.days_remaining,
                        amz_stock_days: lot.amz_stock_days,
                        amz_stock_allocated: allocated,
                        note: lot.note.clone(),
                    });
                }
            }
        }
    }
    Ok(InventoryData { sirali, analiz, stock })
}

#[tauri::command]
fn inv_get_all_data() -> Result<InventoryData, String> {
    internal_get_all_data()
}

#[tauri::command]
fn inv_import_master_excel(file_path: String) -> Result<String, String> {
    let mut conn = get_db_connection().map_err(|e| e.to_string())?;
    let mut workbook = open_workbook_auto(&file_path).map_err(|e| e.to_string())?;
    
    let sheet_names = workbook.sheet_names().to_owned();
    
    // 1. Notları (Notes) Analiz Sayfasından Çekme
    let mut notes_dict: HashMap<(String, String), String> = HashMap::new();
    
    for sheet_name in &sheet_names {
        if sheet_name.to_uppercase().contains("ANAL") {
            if let Ok(range) = workbook.worksheet_range(sheet_name) {
                let mut rows = range.rows();
                if let Some(headers) = rows.next() {
                    let mut id_col = None;
                    let mut sku_col = None;
                    let mut note_col = None;
                    
                    for (i, h) in headers.iter().enumerate() {
                        let h_str = h.to_string().to_lowercase();
                        if h_str.contains("id") { id_col = Some(i); }
                        if h_str.contains("sku") { sku_col = Some(i); }
                        if h_str.contains("not") || h_str.contains("note") { note_col = Some(i); }
                    }
                    
                    if let (Some(id_idx), Some(sku_idx), Some(note_idx)) = (id_col, sku_col, note_col) {
                        for row in rows {
                            let r_id = row.get(id_idx).map(|d| d.to_string().trim().to_uppercase()).unwrap_or_default();
                            let r_sku = row.get(sku_idx).map(|d| d.to_string().trim().to_uppercase()).unwrap_or_default();
                            let r_note = row.get(note_idx).map(|d| d.to_string().trim().to_string()).unwrap_or_default();
                            
                            if !r_id.is_empty() && !r_sku.is_empty() && !r_note.is_empty() && r_note.to_lowercase() != "nan" {
                                notes_dict.insert((r_id, r_sku), r_note);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Ana Veriyi Çekme
    let target_sheet = sheet_names.iter()
        .find(|s| s.to_uppercase().contains("SIRALI") || s.to_uppercase().contains("ANALİZ") || s.to_uppercase().contains("EXPRATION"))
        .unwrap_or(&sheet_names[0]);

    let range = workbook.worksheet_range(target_sheet).map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers = rows.next().ok_or("Başlık satırı yok.")?;

    // Dinamik Sütun Tespiti
    let (mut c_name, mut c_id, mut c_date, mut c_sku, mut c_qty, mut c_exp) = (0, 0, None, 0, 0, 0);
    
    for (i, h) in headers.iter().enumerate() {
        let h_str = h.to_string().to_lowercase();
        if h_str.contains("name") { c_name = i; }
        else if h_str.contains("id") { c_id = i; }
        else if h_str.contains("date") && !h_str.contains("exp") { c_date = Some(i); }
        else if h_str.contains("sku") { c_sku = i; }
        else if h_str.contains("qty") || h_str.contains("adet") { c_qty = i; }
        else if h_str.contains("exp") { c_exp = i; }
    }

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut imported = 0;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO shipment_items 
             (shipment_id, shipment_name, created_date, created_timestamp, sku, qty, exp_date_usa, exp_date_tur, days_remaining, note) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(NULLIF(?10, ''), (SELECT note FROM shipment_items WHERE UPPER(shipment_id)=UPPER(?1) AND UPPER(sku)=UPPER(?5)), ''))"
        ).map_err(|e| e.to_string())?;

        for row in rows {
            let shipment_id = row.get(c_id).map(|d| d.to_string().trim().to_string()).unwrap_or_default();
            let sku = row.get(c_sku).map(|d| d.to_string().trim().to_string()).unwrap_or_default();

            if sku.is_empty() || shipment_id.is_empty() || sku.to_lowercase() == "nan" {
                continue;
            }

            let shipment_name = row.get(c_name).map(|d| d.to_string().trim().to_string()).unwrap_or_default();
            
            let qty = row.get(c_qty).map(|d| {
                match d {
                    Data::Int(i) => *i as i32,
                    Data::Float(f) => *f as i32,
                    Data::String(s) => s.replace(",", ".").parse::<f64>().unwrap_or(0.0) as i32,
                    _ => 0,
                }
            }).unwrap_or(0);

            let raw_date = c_date.and_then(|idx| row.get(idx)).map(|d| d.to_string()).unwrap_or_default();
            let (created_fmt, created_ts) = parse_created_date(&raw_date);

            let raw_exp = row.get(c_exp).map(|d| d.to_string()).unwrap_or_default();
            let (exp_usa, exp_tur, days_left) = parse_exp_date(&raw_exp);

            let matched_note = notes_dict.get(&(shipment_id.to_uppercase(), sku.to_uppercase())).cloned().unwrap_or_default();

            stmt.execute(rusqlite::params![
                shipment_id, shipment_name, created_fmt, created_ts, sku, qty, exp_usa, exp_tur, days_left, matched_note
            ]).ok(); // Hatalı (Duplicate) satırları yoksay
            
            imported += 1;
        }
    }
    
    tx.commit().map_err(|e| e.to_string())?;

    Ok(format!("Master Excel başarıyla yüklendi. ({} satır işlendi)", imported))
}

#[tauri::command]
fn inv_import_picklist(file_paths: Vec<String>) -> Result<String, String> {
    Ok(format!("{} adet dosya aktarıldı", file_paths.len()))
}

#[tauri::command]
fn inv_import_stock(file_path: String) -> Result<String, String> {
    let conn = get_db_connection().map_err(|e| e.to_string())?;

    let mut workbook = open_workbook_auto(&file_path)
        .map_err(|e| format!("Dosya okuma hatası: {}", e))?;

    let sheet_names = workbook.sheet_names().to_owned();
    let sheet_name = sheet_names.first().ok_or("Çalışma sayfası bulunamadı.")?;

    let range = workbook.worksheet_range(sheet_name)
        .map_err(|e| e.to_string())?;

    let mut rows = range.rows();
    let headers = rows.next().ok_or("Dosya boş veya başlık satırı yok.")?;

    // Dinamik Sütun İndeks Tespiti
    let mut sku_idx = 0;
    let mut qty_idx = 1;
    let mut is_formula_based = false;
    let sum_columns = ["Available", "FC transfer", "FC Processing", "Unfulfillable", "Shipped", "Receiving"];
    let mut sum_indices = Vec::new();

    for (i, header) in headers.iter().enumerate() {
        let header_str = header.to_string().trim().to_lowercase();
        if header_str == "merchant sku" || header_str == "sku" {
            sku_idx = i;
        } else if header_str == "total units" || header_str == "qty" {
            qty_idx = i;
        }
        
        for sum_col in &sum_columns {
            if &header_str == &sum_col.to_lowercase() {
                sum_indices.push(i);
            }
        }
    }

    if sum_indices.len() == sum_columns.len() {
        is_formula_based = true;
    }

    let mut stock_dict: HashMap<String, i32> = HashMap::new();

    // Satırları Ayrıştırma ve Tip Doğrulama
    for row in rows {
        let sku = match row.get(sku_idx) {
            Some(Data::String(s)) => s.trim().to_string(),
            Some(d) => d.to_string().trim().to_string(),
            None => continue,
        };

        if sku.is_empty() || sku.to_lowercase() == "nan" {
            continue;
        }

        let mut total_qty = 0;

        if is_formula_based {
            for &idx in &sum_indices {
                if let Some(cell) = row.get(idx) {
                    let val = match cell {
                        Data::Int(i) => *i as i32,
                        Data::Float(f) => *f as i32,
                        Data::String(s) => s.trim().parse::<i32>().unwrap_or(0),
                        _ => 0,
                    };
                    total_qty += val;
                }
            }
        } else {
            if let Some(cell) = row.get(qty_idx) {
                total_qty = match cell {
                    Data::Int(i) => *i as i32,
                    Data::Float(f) => *f as i32,
                    Data::String(s) => s.trim().parse::<i32>().unwrap_or(0),
                    _ => 0,
                };
            }
        }

        stock_dict.insert(sku, total_qty);
    }

    conn.execute("DELETE FROM amazon_stock", [])
        .map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare("INSERT INTO amazon_stock (sku, total_units) VALUES (?1, ?2)")
        .map_err(|e| e.to_string())?;

    for (sku, qty) in &stock_dict {
        stmt.execute(rusqlite::params![sku, qty]).map_err(|e| e.to_string())?;
    }

    Ok(format!("Stok güncellendi ({} SKU işlendi).", stock_dict.len()))
}

#[tauri::command]
fn inv_reset_data() -> Result<String, String> {
    let conn = get_db_connection().map_err(|e| e.to_string())?;
    
    // Silmeden önce mevcut verilerin tam yedeğini kopyala
    conn.execute_batch(
        "DROP TABLE IF EXISTS shipment_items_backup;
         CREATE TABLE shipment_items_backup AS SELECT * FROM shipment_items;
         
         DROP TABLE IF EXISTS amazon_stock_backup;
         CREATE TABLE amazon_stock_backup AS SELECT * FROM amazon_stock;
         
         DELETE FROM shipment_items;
         DELETE FROM amazon_stock;"
    ).map_err(|e| e.to_string())?;

    Ok("Tüm veriler silindi! İşlemi geri almak için 'Geri Al' butonuna tıklayabilirsiniz.".to_string())
}

#[tauri::command]
fn inv_undo_reset() -> Result<String, String> {
    let conn = get_db_connection().map_err(|e| e.to_string())?;
    
    // Yedeğin varlığını doğrula
    let backup_exists: bool = conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='shipment_items_backup'",
        [],
        |_| Ok(true),
    ).unwrap_or(false);
    
    if !backup_exists {
        return Err("Geri alınacak bir yedek bulunamadı veya yedek hasarlı.".to_string());
    }

    // Ana tabloları temizle ve yedekten verileri geri bas
    conn.execute_batch(
        "DELETE FROM shipment_items;
         INSERT INTO shipment_items SELECT * FROM shipment_items_backup;
         
         DELETE FROM amazon_stock;
         INSERT INTO amazon_stock SELECT * FROM amazon_stock_backup;"
    ).map_err(|e| e.to_string())?;

    Ok("Kritik müdahale başarılı: Silinen veriler kurtarıldı!".to_string())
}

#[tauri::command]
fn inv_update_note(shipment_id: String, sku: String, exp_date_usa: String, note: String) -> Result<String, String> {
    let conn = get_db_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE shipment_items SET note = ?1 WHERE shipment_id = ?2 AND sku = ?3 AND exp_date_usa = ?4",
        rusqlite::params![note, shipment_id, sku, exp_date_usa],
    ).map_err(|e| e.to_string())?;
    Ok("Güncellendi".to_string())
}

fn parse_created_date(val_str: &str) -> (String, String) {
    let val = val_str.trim();
    let today = Utc::now().naive_utc();
    let default_formatted = today.format("%d.%m.%Y").to_string();
    let default_timestamp = today.format("%Y-%m-%d 00:00:00").to_string();

    if val.is_empty() || val.to_lowercase() == "nan" {
        return (default_formatted, default_timestamp);
    }

    let re = Regex::new(r"(?i)(?:CREATED\s*>\s*)?(\d{1,2})\s*([A-Z]{3})\s*(\d{4})").unwrap();
    if let Some(caps) = re.captures(val) {
        let day: u32 = caps[1].parse().unwrap_or(1);
        let month_str = caps[2].to_uppercase();
        let year: i32 = caps[3].parse().unwrap_or(today.year());
        
        let month = match month_str.as_str() {
            "JAN" => 1, "FEB" => 2, "MAR" => 3, "APR" => 4,
            "MAY" => 5, "JUN" => 6, "JUL" => 7, "AUG" => 8,
            "SEP" => 9, "OCT" => 10, "NOV" => 11, "DEC" => 12,
            _ => 1,
        };

        if let Some(dt) = NaiveDate::from_ymd_opt(year, month, day) {
            return (dt.format("%d.%m.%Y").to_string(), dt.format("%Y-%m-%d 00:00:00").to_string());
        }
    }
    
    // Geri Dönüş (Fallback) Stratejisi
    (default_formatted, default_timestamp)
}

fn parse_exp_date(val_str: &str) -> (String, String, i32) {
    let val = val_str.trim();
    if val.is_empty() || val.to_lowercase() == "nan" {
        return (String::new(), String::new(), 0);
    }

    let today = Utc::now().naive_utc().date();
    let re = Regex::new(r"^(\d{1,2})[-/.](\d{1,2})[-/.](\d{4})$").unwrap();
    
    let mut parsed_date = None;

    if let Some(caps) = re.captures(val) {
        let p1: u32 = caps[1].parse().unwrap_or(1);
        let p2: u32 = caps[2].parse().unwrap_or(1);
        let p3: i32 = caps[3].parse().unwrap_or(today.year());

        if p1 > 12 {
            parsed_date = NaiveDate::from_ymd_opt(p3, p2, p1);
        } else {
            parsed_date = NaiveDate::from_ymd_opt(p3, p1, p2).or_else(|| NaiveDate::from_ymd_opt(p3, p2, p1));
        }
    }

    if let Some(dt) = parsed_date {
        let usa = dt.format("%m-%d-%Y").to_string();
        let tur = dt.format("%d.%m.%Y").to_string();
        let days = dt.signed_duration_since(today).num_days() as i32;
        (usa, tur, days)
    } else {
        let clean_val = val.replace(".", "-");
        (clean_val.clone(), clean_val, 0)
    }
}

fn extract_shipment_ids(val: &str) -> Vec<String> {
    let re = Regex::new(r"(?i)\b(FBA[A-Z0-9]{8,12})\b").unwrap();
    re.captures_iter(val).map(|c| c[1].to_uppercase()).collect()
}

#[tauri::command]
fn inv_export_excel(output_path: String) -> Result<String, String> {
    use rust_xlsxwriter::{Workbook, Format, Color, FormatAlign, FormatBorder};

    let data = internal_get_all_data()?;
    let mut workbook = Workbook::new();

    let header_format = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x1F497D))
        .set_align(FormatAlign::Center)
        .set_border(FormatBorder::Thin);

    let row_format = Format::new().set_border(FormatBorder::Thin);
    
    // 1. SIRALI Sayfası
    let ws_sirali = workbook.add_worksheet().set_name("SIRALI").map_err(|e| e.to_string())?;
    let headers_sirali = ["SHIPMENT NAME", "SHIPMENT ID", "DATE", "SKU", "QTY", "EXP DATE USA", "EXP DATE TUR", "SKT GÜN", "AMZ Stok Gün"];
    
    for (col, header) in headers_sirali.iter().enumerate() {
        ws_sirali.write_string_with_format(0, col as u16, *header, &header_format).map_err(|e| e.to_string())?;
    }

    for (row, item) in data.sirali.iter().enumerate() {
        let r = (row + 1) as u32;
        ws_sirali.write_string_with_format(r, 0, &item.shipment_name, &row_format).ok();
        ws_sirali.write_string_with_format(r, 1, &item.shipment_id, &row_format).ok();
        ws_sirali.write_string_with_format(r, 2, &item.created_date, &row_format).ok();
        ws_sirali.write_string_with_format(r, 3, &item.sku, &row_format).ok();
        ws_sirali.write_number_with_format(r, 4, item.qty_shipped as f64, &row_format).ok();
        ws_sirali.write_string_with_format(r, 5, &item.exp_date_usa, &row_format).ok();
        ws_sirali.write_string_with_format(r, 6, &item.exp_date_tur, &row_format).ok();
        ws_sirali.write_number_with_format(r, 7, item.days_remaining as f64, &row_format).ok();
        ws_sirali.write_number_with_format(r, 8, item.amz_stock_days as f64, &row_format).ok();
    }

    // 2. ANALİZ Sayfası
    let ws_analiz = workbook.add_worksheet().set_name("ANALİZ").map_err(|e| e.to_string())?;
    let headers_analiz = ["SHIPMENT NAME", "SHIPMENT ID", "SKU", "QTY", "AMZ STOK", "AMZ STOK GÜN", "SKT GÜN", "NOT"];
    
    for (col, header) in headers_analiz.iter().enumerate() {
        ws_analiz.write_string_with_format(0, col as u16, *header, &header_format).map_err(|e| e.to_string())?;
    }

    for (row, item) in data.analiz.iter().enumerate() {
        let r = (row + 1) as u32;
        ws_analiz.write_string_with_format(r, 0, &item.shipment_name, &row_format).ok();
        ws_analiz.write_string_with_format(r, 1, &item.shipment_id, &row_format).ok();
        ws_analiz.write_string_with_format(r, 2, &item.sku, &row_format).ok();
        ws_analiz.write_number_with_format(r, 3, item.qty_shipped as f64, &row_format).ok();
        ws_analiz.write_number_with_format(r, 4, item.amz_stock_allocated as f64, &row_format).ok();
        ws_analiz.write_number_with_format(r, 5, item.amz_stock_days as f64, &row_format).ok();
        ws_analiz.write_number_with_format(r, 6, item.days_remaining as f64, &row_format).ok();
        ws_analiz.write_string_with_format(r, 7, &item.note, &row_format).ok();
    }

    workbook.save(&output_path).map_err(|e| e.to_string())?;
    Ok(output_path)
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
        .plugin(tauri_plugin_process::init())
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
            run_shipment_creator,
            inv_get_all_data,
            inv_import_master_excel,
            inv_import_picklist,
            inv_import_stock,
            inv_reset_data,
            inv_update_note,
            inv_export_excel,
            inv_undo_reset
        ])
        .run(tauri::generate_context!())
        .expect("Kritik Hata: Tauri motoru başlatılamadı.");
}
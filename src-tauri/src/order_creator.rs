use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use rayon::prelude::*;

fn get_col_idx(headers: &[String], possible_names: Option<&Vec<Value>>) -> Result<usize, String> {
    let names = possible_names.ok_or("Ayarlarda sütun listesi eksik.")?;
    for val in names {
        if let Some(name) = val.as_str() {
            if let Some(idx) = headers.iter().position(|h| h.eq_ignore_ascii_case(name)) {
                return Ok(idx);
            }
        }
    }
    Err(format!("Beklenen sütunlardan hiçbiri bulunamadı: {:?}", names))
}

fn clean_upc(cell: &Data) -> String {
    let s = cell.to_string();
    let s = s.trim();
    if let Ok(num) = s.parse::<f64>() {
        if num.fract() == 0.0 {
            return format!("{:.0}", num);
        } else {
            return format!("{}", num);
        }
    }
    s.to_string()
}

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    restock_files: Vec<String>,
    orderform_files: Vec<String>,
    template_path: String,
    output_folder: String,
    settings: Value,
) -> Result<String, String> {
    if restock_files.is_empty() { return Err("Hata: Restock excel dosyası sağlanmadı.".to_string()); }
    if orderform_files.is_empty() { return Err("Hata: Order Form excel dosyası sağlanmadı.".to_string()); }
    if !Path::new(&template_path).exists() { return Err(format!("Hata: Template dosyası bulunamadı -> {}", template_path)); }

    let r_cols = settings.get("restock_columns").and_then(|v| v.as_object()).ok_or("restock_columns ayarı eksik.")?;
    let o_cols = settings.get("orderform_columns").and_then(|v| v.as_object()).ok_or("orderform_columns ayarı eksik.")?;

    let mut aggregated_data: HashMap<String, HashMap<String, f64>> = HashMap::new(); // Target -> UPC -> PCS

    // 1. RESTOCK İŞLEMLERİ
    app.emit("job-log", serde_json::json!({ "message": "Restock dosyası okunuyor...", "percent": 10 })).unwrap_or(());
    
    let mut wb = open_workbook_auto(&restock_files[0]).map_err(|e| e.to_string())?;
    let sheet = wb.sheet_names().first().unwrap().clone();
    let range = wb.worksheet_range(&sheet).map_err(|e| e.to_string())?;
    let mut iter = range.rows();
    
    let header_row = iter.next().ok_or("Restock başlığı bulunamadı.")?;
    let r_headers: Vec<String> = header_row.iter().map(|c| c.to_string().trim().to_string()).collect();

    let r_upc_idx = get_col_idx(&r_headers, r_cols.get("upc").and_then(|v| v.as_array()))?;
    let r_pcs_idx = get_col_idx(&r_headers, r_cols.get("pcs").and_then(|v| v.as_array()))?;
    let r_sup_idx = get_col_idx(&r_headers, r_cols.get("suplier").and_then(|v| v.as_array()))?;
    let r_notes_idx = get_col_idx(&r_headers, r_cols.get("notes").and_then(|v| v.as_array()))?;

    for row in iter {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
        let upc = clean_upc(row.get(r_upc_idx).unwrap_or(&Data::Empty));
        if upc.is_empty() { continue; }

        let pcs = row.get(r_pcs_idx).unwrap_or(&Data::Empty).to_string().replace(",", ".").parse::<f64>().unwrap_or(0.0);
        if pcs == 0.0 { continue; }

        let supplier = row.get(r_sup_idx).unwrap_or(&Data::Empty).to_string().trim().to_string();
        if !supplier.is_empty() {
            *aggregated_data.entry(supplier).or_default().entry(upc.clone()).or_insert(0.0) += pcs;
        }

        let note = row.get(r_notes_idx).unwrap_or(&Data::Empty).to_string().trim().to_string();
        if !note.is_empty() && note != "0" && note != "0.0" {
            *aggregated_data.entry(note).or_default().entry(upc.clone()).or_insert(0.0) += pcs;
        }
    }

    // 2. ORDER FORM İŞLEMLERİ
    app.emit("job-log", serde_json::json!({ "message": "Order Form dosyası okunuyor...", "percent": 40 })).unwrap_or(());
    
    let mut wb_order = open_workbook_auto(&orderform_files[0]).map_err(|e| e.to_string())?;
    let sheet_order = wb_order.sheet_names().first().unwrap().clone();
    let range_order = wb_order.worksheet_range(&sheet_order).map_err(|e| e.to_string())?;
    let mut iter_order = range_order.rows();
    
    let header_order = iter_order.next().ok_or("Order Form başlığı bulunamadı.")?;
    let o_headers: Vec<String> = header_order.iter().map(|c| c.to_string().trim().to_string()).collect();

    let o_upc_idx = get_col_idx(&o_headers, o_cols.get("upc").and_then(|v| v.as_array()))?;
    let o_pcs_idx = get_col_idx(&o_headers, o_cols.get("pcs").and_then(|v| v.as_array()))?;
    let o_sup_idx = get_col_idx(&o_headers, o_cols.get("suplier").and_then(|v| v.as_array()))?;

    for row in iter_order {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
        let upc = clean_upc(row.get(o_upc_idx).unwrap_or(&Data::Empty));
        if upc.is_empty() { continue; }

        let pcs = row.get(o_pcs_idx).unwrap_or(&Data::Empty).to_string().replace(",", ".").parse::<f64>().unwrap_or(0.0);
        if pcs == 0.0 { continue; }

        let supplier = row.get(o_sup_idx).unwrap_or(&Data::Empty).to_string().trim().to_string();
        if !supplier.is_empty() {
            *aggregated_data.entry(supplier).or_default().entry(upc.clone()).or_insert(0.0) += pcs;
        }
    }

    // 3. ŞABLONA YAZDIRMA (Gerçek Paralelizasyon - Rayon)
    app.emit("job-log", serde_json::json!({ "message": "Şablonlara yazdırılıyor...", "percent": 70 })).unwrap_or(());
    let target_dir = Path::new(&output_folder).join("ORDERS");
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    // Hashmap'i bir vektöre dönüştürüp, her tedarikçi (Target) için dosya yazma işlemini paralel Thread'lere (Çekirdeklere) dağıtıyoruz.
    let groups: Vec<_> = aggregated_data.into_iter().collect();
    
    let write_results: Result<Vec<_>, String> = groups.par_iter().map(|(target, upc_map)| {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
        
        let safe_target = target.replace("/", "-").replace("\\", "-").to_uppercase();
        let output_path = target_dir.join(format!("{}.xlsx", safe_target));
        
        // calamine okuyucudur, ancak rust_xlsxwriter sıfırdan oluşturur. 
        // Şablon dosyaları genelde başlık içerir. Kopyalamak yerine hız için bellekte başlık atıp verileri yazarız.
        // Eğer template dosyası özel formüller içeriyorsa 'xlsx_writer' yerine farklı yaklaşım gerekir ancak Python'daki openpyxl sadece raw data yazmıştır.
        
        let mut wb_out = Workbook::new();
        let ws = wb_out.add_worksheet();
        
        // 1. Sütun forması (String / UPC)
        let upc_format = Format::new().set_num_format("000000000000");

        // Python'daki kodda start_row 2 (Yani 1. index). Biz de A1 ve C1'e başlık atıp verileri 2. satırdan (index 1) yazıyoruz.
        ws.write_string(0, 0, "UPC").map_err(|e| e.to_string())?;
        ws.write_string(0, 2, "PCS").map_err(|e| e.to_string())?;

        let mut row_idx = 1;
        for (upc, pcs) in upc_map {
            ws.write_string_with_format(row_idx, 0, upc, &upc_format).map_err(|e| e.to_string())?;
            ws.write_number(row_idx, 2, *pcs).map_err(|e| e.to_string())?;
            row_idx += 1;
        }

        wb_out.save(&output_path).map_err(|e| format!("Dosya kaydedilemedi ({}): {}", safe_target, e))?;
        Ok(())
    }).collect();

    write_results?;

    app.emit("job-log", serde_json::json!({ "message": "Order Create işlemi tamamlandı!", "percent": 100 })).unwrap_or(());
    Ok(target_dir.to_string_lossy().to_string())
}
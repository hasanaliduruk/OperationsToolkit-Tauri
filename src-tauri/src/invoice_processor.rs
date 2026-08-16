use chrono::{NaiveDate, NaiveDateTime};
use csv::{ReaderBuilder, StringRecord};
use rust_xlsxwriter::Workbook;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

fn find_col_idx(header: &StringRecord, possible_names: Option<&Vec<Value>>) -> Result<usize, String> {
    let names = possible_names.ok_or("Sütun ayarı eksik.")?;
    for val in names {
        if let Some(name) = val.as_str() {
            if let Some(idx) = header.iter().position(|h| h.trim().eq_ignore_ascii_case(name)) {
                return Ok(idx);
            }
        }
    }
    Err("Beklenen sütun CSV dosyasında bulunamadı.".to_string())
}

fn format_date(raw_date: &str) -> String {
    let clean = raw_date.replace(",", "/").replace("-", "/");
    let date_formats = ["%m/%d/%Y", "%m/%d/%y", "%Y/%m/%d", "%d/%m/%Y"];
    let datetime_formats = ["%Y/%m/%d %H:%M:%S", "%m/%d/%Y %H:%M:%S"];

    for f in date_formats.iter() {
        if let Ok(parsed) = NaiveDate::parse_from_str(&clean, f) {
            return parsed.format("%m/%d/%Y").to_string();
        }
    }
    for f in datetime_formats.iter() {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(&clean, f) {
            return parsed.format("%m/%d/%Y").to_string();
        }
    }
    
    if clean.trim().is_empty() { "#HATA".to_string() } else { clean }
}

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    input_files: Vec<String>,
    output_folder: String,
    settings: Value,
    del_zero: bool,
) -> Result<String, String> {
    if input_files.is_empty() { return Err("İşlenecek dosya bulunamadı.".to_string()); }

    let columns_dict = settings.get("columns").and_then(|v| v.as_object()).ok_or("Ayarlar bozuk veya eksik.")?;
    let remove_cols: Vec<&str> = columns_dict.get("remove")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let mut row_idx: u32 = 0;

    let mut master_header: Vec<String> = Vec::new();

    for (file_index, file_path) in input_files.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
        let progress = (file_index as f64 / input_files.len() as f64 * 90.0) as u32;
        let file_name = Path::new(file_path).file_name().unwrap_or_default().to_string_lossy();
        app.emit("job-log", serde_json::json!({ "message": format!("Dosya işleniyor: {}", file_name), "percent": progress })).unwrap_or(());

        if !Path::new(file_path).exists() { continue; }
        
        let mut rdr = ReaderBuilder::new().has_headers(true).from_path(file_path).map_err(|e| e.to_string())?;
        let header = rdr.headers().map_err(|e| e.to_string())?.clone();

        let sq_idx = find_col_idx(&header, columns_dict.get("shipquantity").and_then(|v| v.as_array()))
            .map_err(|_| format!("'shipquantity' sütunu {} dosyasında bulunamadı.", file_name))?;
        let date_idx = find_col_idx(&header, columns_dict.get("date").and_then(|v| v.as_array()))
            .map_err(|_| format!("'date' sütunu {} dosyasında bulunamadı.", file_name))?;

        let mut current_mapping = Vec::new();

        if file_index == 0 {
            let mut out_col_idx: u16 = 0;
            for (i, h) in header.iter().enumerate() {
                if !remove_cols.contains(&h.trim()) {
                    master_header.push(h.trim().to_string());
                    current_mapping.push(i);
                    worksheet.write_string(row_idx, out_col_idx, h.trim()).map_err(|e| e.to_string())?;
                    out_col_idx += 1;
                }
            }
            row_idx += 1;
        } else {
            for mh in &master_header {
                if let Some(idx) = header.iter().position(|h| h.trim() == mh) {
                    current_mapping.push(idx);
                } else {
                    return Err(format!("Sütun uyumsuzluğu: {} dosyası '{}' sütununu içermiyor.", file_name, mh));
                }
            }
        }

        for result in rdr.records() {
            if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
            let record = result.map_err(|e| e.to_string())?;
            
            let sq_str = record.get(sq_idx).unwrap_or("0").trim();
            let sq_val = sq_str.parse::<f64>().unwrap_or(0.0);
            if del_zero && sq_val == 0.0 { continue; }

            let mut out_col: u16 = 0;
            for &in_idx in &current_mapping {
                let mut val = record.get(in_idx).unwrap_or("").to_string();
                if in_idx == date_idx { val = format_date(&val); }
                if in_idx == sq_idx {
                    worksheet.write_number(row_idx, out_col, sq_val).map_err(|e| e.to_string())?;
                } else {
                    worksheet.write_string(row_idx, out_col, &val).map_err(|e| e.to_string())?;
                }
                out_col += 1;
            }
            row_idx += 1;
        }
    }

    app.emit("job-log", serde_json::json!({ "message": "Excel dosyası diske kaydediliyor...", "percent": 100 })).unwrap_or(());
    let target_dir = Path::new(&output_folder).join("invoice_sonuc_excel");
    fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    
    let output_path = target_dir.join("toplu.xlsx");
    workbook.save(&output_path).map_err(|e| e.to_string())?;

    Ok(output_path.to_string_lossy().to_string())
}
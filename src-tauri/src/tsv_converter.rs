use rust_xlsxwriter::Workbook;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    files: Vec<String>,
    target_path: String,
) -> Result<String, String> {
    if files.is_empty() {
        return Err("Hata: İşlenecek dosya listesi boş.".to_string());
    }

    let mut aggregated: HashMap<String, f64> = HashMap::new();

    for (idx, file_path) in files.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem kullanıcı tarafından iptal edildi.".to_string()); }
        
        let progress = (idx as f64 / files.len() as f64 * 60.0) as u32;
        let file_name = Path::new(file_path).file_name().unwrap_or_default().to_string_lossy();
        app.emit("job-log", serde_json::json!({ "message": format!("Belleğe alınıyor: {}", file_name), "percent": progress })).unwrap_or(());

        let file = File::open(file_path).map_err(|e| format!("Dosya açılamadı: {}", e))?;
        let reader = BufReader::new(file);

        let mut header_found = false;
        let mut sku_idx = 0;
        let mut shipped_idx = 0;

        for line_result in reader.lines() {
            let line = line_result.unwrap_or_default();
            if line.trim().is_empty() { continue; }
            
            let cols: Vec<&str> = line.split('\t').collect();
            
            if !header_found {
                if cols.contains(&"Merchant SKU") && cols.contains(&"Shipped") {
                    sku_idx = cols.iter().position(|&r| r == "Merchant SKU").unwrap();
                    shipped_idx = cols.iter().position(|&r| r == "Shipped").unwrap();
                    header_found = true;
                }
                continue;
            }

            if cols.len() > sku_idx && cols.len() > shipped_idx {
                let sku = cols[sku_idx].trim().to_string();
                if let Ok(val) = cols[shipped_idx].trim().parse::<f64>() {
                    *aggregated.entry(sku).or_insert(0.0) += val;
                }
            }
        }
    }

    if aggregated.is_empty() {
        return Err("Hata: Hiçbir veriden geçerli 'Merchant SKU' ve 'Shipped' eşleşmesi çıkarılamadı.".to_string());
    }

    app.emit("job-log", serde_json::json!({ "message": "Excel dosyası oluşturuluyor...", "percent": 80 })).unwrap_or(());

    std::fs::create_dir_all(&target_path).map_err(|e| e.to_string())?;
    
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    
    worksheet.write_string(0, 0, "Merchant SKU").map_err(|e| e.to_string())?;
    worksheet.write_string(0, 1, "Shipped").map_err(|e| e.to_string())?;

    let mut row = 1;
    for (sku, qty) in aggregated {
        worksheet.write_string(row, 0, &sku).map_err(|e| e.to_string())?;
        worksheet.write_number(row, 1, qty).map_err(|e| e.to_string())?;
        row += 1;
    }

    let out_path = Path::new(&target_path).join("son.xlsx");
    workbook.save(&out_path).map_err(|e| format!("Excel kaydedilemedi: {}", e))?;

    app.emit("job-log", serde_json::json!({ "message": "İşlem tamamlandı.", "percent": 100 })).unwrap_or(());
    Ok(out_path.to_string_lossy().to_string())
}
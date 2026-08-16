use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::Workbook;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    path: String,
    name: String,
    restock_excel: String,
    future_excel: String,
) -> Result<String, String> {
    app.emit("job-log", serde_json::json!({ "message": "Restock dosyası okunuyor...", "percent": 10 })).unwrap_or(());
    if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }

    let mut restock_wb = open_workbook_auto(&restock_excel).map_err(|e| e.to_string())?;
    let r_sheets = restock_wb.sheet_names().to_owned();
    let r_sheet = r_sheets.first().ok_or("Restock Excel dosyası boş.")?;
    let r_range = restock_wb.worksheet_range(r_sheet).map_err(|e| e.to_string())?;

    let mut r_rows = r_range.rows();
    let r_header = r_rows.next().ok_or("Restock dosyasında başlık bulunamadı.")?;
    let r_headers: Vec<String> = r_header.iter().map(|c| c.to_string().trim().to_string()).collect();
    let r_asin_idx = r_headers.iter().position(|h| h == "ASIN").ok_or("Restock dosyasında 'ASIN' sütunu bulunamadı.")?;

    app.emit("job-log", serde_json::json!({ "message": "Future Price dosyası okunuyor...", "percent": 30 })).unwrap_or(());
    if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }

    let mut future_wb = open_workbook_auto(&future_excel).map_err(|e| e.to_string())?;
    let f_sheets = future_wb.sheet_names().to_owned();
    let f_sheet = f_sheets.first().ok_or("Future Excel dosyası boş.")?;
    let f_range = future_wb.worksheet_range(f_sheet).map_err(|e| e.to_string())?;

    let mut f_rows = f_range.rows();
    let f_header = f_rows.next().ok_or("Future dosyasında başlık bulunamadı.")?;
    let f_headers: Vec<String> = f_header.iter().map(|c| c.to_string().trim().to_string()).collect();
    let f_asin_idx = f_headers.iter().position(|h| h == "ASIN").ok_or("Future dosyasında 'ASIN' sütunu bulunamadı.")?;

    let mut f_data: HashMap<String, HashMap<String, String>> = HashMap::new();
    for row in f_rows {
        let asin = row.get(f_asin_idx).unwrap_or(&Data::Empty).to_string();
        if asin.is_empty() { continue; }
        
        let mut row_map = HashMap::new();
        for (i, cell) in row.iter().enumerate() {
            row_map.insert(f_headers[i].clone(), cell.to_string());
        }
        f_data.insert(asin, row_map);
    }

    let mut f_suppliers_lower = HashMap::new();
    for h in &f_headers {
        let hl = h.to_lowercase();
        if hl.ends_with(" price") && hl != "future price" {
            let sup = h[..h.len() - 6].trim().to_string();
            f_suppliers_lower.insert(sup.to_lowercase(), sup);
        }
    }

    let mut valid_r_suppliers = Vec::new();
    for h in &r_headers {
        let hl = h.to_lowercase();
        if hl.ends_with(" price") && hl != "future price" {
            let sup = h[..h.len() - 6].trim().to_string();
            if f_suppliers_lower.contains_key(&sup.to_lowercase()) {
                valid_r_suppliers.push(sup);
            }
        }
    }

    let mut final_headers = Vec::new();
    for h in &r_headers {
        final_headers.push(h.clone());
        if h == "Price" && f_headers.contains(&"Price".to_string()) {
            final_headers.push("Future Price".to_string());
        } else if h == "Maliyet" && f_headers.contains(&"Maliyet".to_string()) {
            final_headers.push("Future Maliyet".to_string());
        } else {
            for r_sup in &valid_r_suppliers {
                if h == &format!("{} price", r_sup) {
                    let f_sup = f_suppliers_lower.get(&r_sup.to_lowercase()).unwrap();
                    let f_col = format!("{} price", f_sup);
                    if f_headers.contains(&f_col) {
                        final_headers.push(format!("{} future price", r_sup));
                    }
                }
            }
        }
    }

    app.emit("job-log", serde_json::json!({ "message": "Eşleştirme yapılıyor ve diske yazılıyor...", "percent": 60 })).unwrap_or(());

    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let out_name = if name.trim().is_empty() { "Future_Price_Sonuc".to_string() } else { name };
    let out_path = Path::new(&path).join(format!("{}.xlsx", out_name));

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();

    for (c, h) in final_headers.iter().enumerate() {
        ws.write_string(0, c as u16, h).map_err(|e| e.to_string())?;
    }

    let mut r_idx = 1;
    for row in r_range.rows().skip(1) {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }

        let asin = row.get(r_asin_idx).unwrap_or(&Data::Empty).to_string();
        let mut out_col = 0;
        
        for h in &r_headers {
            let cell_val = row.get(r_headers.iter().position(|x| x == h).unwrap()).unwrap_or(&Data::Empty).to_string();
            ws.write_string(r_idx, out_col, &cell_val).map_err(|e| e.to_string())?;
            out_col += 1;

            if h == "Price" && f_headers.contains(&"Price".to_string()) {
                let val = f_data.get(&asin).and_then(|m| m.get("Price")).cloned().unwrap_or_else(|| "#YOK".to_string());
                ws.write_string(r_idx, out_col, &val).map_err(|e| e.to_string())?;
                out_col += 1;
            } else if h == "Maliyet" && f_headers.contains(&"Maliyet".to_string()) {
                let val = f_data.get(&asin).and_then(|m| m.get("Maliyet")).cloned().unwrap_or_else(|| "#YOK".to_string());
                ws.write_string(r_idx, out_col, &val).map_err(|e| e.to_string())?;
                out_col += 1;
            } else {
                for r_sup in &valid_r_suppliers {
                    if h == &format!("{} price", r_sup) {
                        let f_sup = f_suppliers_lower.get(&r_sup.to_lowercase()).unwrap();
                        let f_col = format!("{} price", f_sup);
                        if f_headers.contains(&f_col) {
                            let val = f_data.get(&asin).and_then(|m| m.get(&f_col)).cloned().unwrap_or_else(|| "#YOK".to_string());
                            ws.write_string(r_idx, out_col, &val).map_err(|e| e.to_string())?;
                            out_col += 1;
                        }
                    }
                }
            }
        }
        r_idx += 1;
    }

    wb.save(&out_path).map_err(|e| e.to_string())?;
    app.emit("job-log", serde_json::json!({ "message": "İşlem tamamlandı.", "percent": 100 })).unwrap_or(());
    Ok(path)
}
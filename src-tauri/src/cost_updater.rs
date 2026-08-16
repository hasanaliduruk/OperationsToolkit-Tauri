use csv::{ReaderBuilder, StringRecord, WriterBuilder};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

fn find_col(header: &StringRecord, possible_names: Option<&Vec<Value>>) -> Result<usize, String> {
    let names = possible_names.ok_or("Ayarlarda sütun listesi bulunamadı.")?;
    for val in names {
        if let Some(name) = val.as_str() {
            if let Some(idx) = header.iter().position(|h| h.trim() == name) {
                return Ok(idx);
            }
        }
    }
    Err(format!("Beklenen sütunlardan hiçbiri bulunamadı: {:?}", names))
}

fn extract_price(sku: &str) -> Option<f64> {
    let parts: Vec<&str> = sku.split('_').collect();
    let mut price = None;
    for p in parts.iter().skip(1) {
        let p_clean = p.replace(",", ".");
        if let Ok(val) = p_clean.parse::<f64>() {
            price = Some(val);
        }
    }
    price
}

fn equation(code: i64, value: f64) -> f64 {
    if code == 1 {
        if value <= 0.75 { 0.18 }
        else if value <= 1.5 { 0.22 }
        else if value <= 3.0 { 0.27 }
        else { 0.37 }
    } else if code == 2 {
        if value <= 0.75 { 0.34 }
        else if value <= 1.5 { 0.41 }
        else if value <= 3.0 { 0.49 }
        else { 0.68 }
    } else {
        0.0
    }
}

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    input_file: &str,
    output_folder: &str,
    settings: Value,
    version: u8,
) -> Result<String, String> {
    let columns_dict = settings.get("columns").and_then(|v| v.as_object()).ok_or("Ayarlarda 'columns' bulunamadı.")?;
    let wh_dict = settings.get("warehouses").and_then(|v| v.as_object()).ok_or("Ayarlarda 'warehouses' bulunamadı.")?;

    let mut rdr = ReaderBuilder::new().has_headers(true).from_path(input_file).map_err(|e| e.to_string())?;
    let header = rdr.headers().map_err(|e| e.to_string())?.clone();

    let sku_idx = find_col(&header, columns_dict.get("sku").and_then(|v| v.as_array()))?;
    let cost_idx = find_col(&header, columns_dict.get("cost").and_then(|v| v.as_array()))?;
    let add_cost_idx = find_col(&header, columns_dict.get("additional cost").and_then(|v| v.as_array()))?;
    let bp_idx = find_col(&header, columns_dict.get("bp strategy").and_then(|v| v.as_array()))?;
    let qd_idx = find_col(&header, columns_dict.get("qd strategy").and_then(|v| v.as_array()))?;
    let bus_idx = find_col(&header, columns_dict.get("business pricing").and_then(|v| v.as_array()))?;

    let (vol_idx, weight_idx) = if version == 2 {
        (
            find_col(&header, columns_dict.get("pkg volume").and_then(|v| v.as_array()))?,
            find_col(&header, columns_dict.get("pkg weight").and_then(|v| v.as_array()))?,
        )
    } else {
        (0, 0)
    };

    fs::create_dir_all(output_folder).map_err(|e| e.to_string())?;
    let file_name = Path::new(input_file).file_name().unwrap().to_string_lossy();
    let output_path = Path::new(output_folder).join(file_name.as_ref());

    let mut wtr = WriterBuilder::new().from_path(&output_path).map_err(|e| e.to_string())?;
    wtr.write_record(&header).map_err(|e| e.to_string())?;

    for (idx, result) in rdr.records().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem kullanıcı tarafından iptal edildi.".to_string()); }
        if idx % 1000 == 0 {
            app.emit("job-log", serde_json::json!({ "message": format!("Satır {} işleniyor...", idx), "percent": 50 })).unwrap_or(());
        }

        let record = result.map_err(|e| e.to_string())?;
        let sku = record.get(sku_idx).unwrap_or("");
        let dc = sku.split('_').next().unwrap_or("");
        let price = extract_price(sku);

        let mut final_cost = "#YOK".to_string();
        let mut final_add_cost = "#YOK".to_string();

        if version == 1 {
            if let Some(wh_val) = wh_dict.get(dc) {
                final_add_cost = wh_val.as_f64().unwrap_or(0.0).to_string();
            }
            if let Some(p) = price {
                final_cost = p.to_string();
            }
        } else if version == 2 {
            let mut add_c = 0.0;
            let mut eq_ind = 0;
            let mut wh_fee = 0.0;

            if let Some(wh_data) = wh_dict.get(dc).and_then(|v| v.as_object()) {
                add_c = wh_data.get("v2_additional_cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                eq_ind = wh_data.get("v2_equation").and_then(|v| v.as_i64()).unwrap_or(0);
                wh_fee = wh_data.get("v2_warehouse_fee").and_then(|v| v.as_f64()).unwrap_or(0.0);
            }

            let vol = record.get(vol_idx).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
            let weight = record.get(weight_idx).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
            let biggest = (vol / 139.0).max(weight);

            if let Some(p) = price {
                final_cost = format!("{:.2}", p + equation(eq_ind, biggest) + wh_fee);
            }
            final_add_cost = add_c.to_string();
        }

        let mut new_record = StringRecord::new();
        for (i, field) in record.iter().enumerate() {
            if i == cost_idx { new_record.push_field(&final_cost); }
            else if i == add_cost_idx { new_record.push_field(&final_add_cost); }
            else if i == bp_idx { new_record.push_field("AI"); }
            else if i == qd_idx { new_record.push_field("default"); }
            else if i == bus_idx { new_record.push_field("on"); }
            else { new_record.push_field(field); }
        }

        wtr.write_record(&new_record).map_err(|e| e.to_string())?;
    }

    app.emit("job-log", serde_json::json!({ "message": "İşlem tamamlandı, kaydediliyor...", "percent": 100 })).unwrap_or(());
    wtr.flush().map_err(|e| e.to_string())?;
    Ok(output_path.to_string_lossy().to_string())
}
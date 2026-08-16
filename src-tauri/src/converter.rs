use calamine::{open_workbook_auto, Data, Reader};
use csv::{ReaderBuilder, WriterBuilder};
use rust_xlsxwriter::Workbook;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    input_files: Vec<String>,
    output_folder: String,
    input_type: String,
    output_type: String,
) -> Result<String, String> {
    let target_dir = Path::new(&output_folder).join("sonuc_dosyalari");
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    for (idx, file) in input_files.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem kullanıcı tarafından iptal edildi.".to_string()); }
        
        let progress = (idx as f64 / input_files.len() as f64 * 90.0) as u32;
        let file_name = Path::new(file).file_name().unwrap_or_default().to_string_lossy();
        app.emit("job-log", serde_json::json!({ "message": format!("Dönüştürülüyor: {}", file_name), "percent": progress })).unwrap_or(());

        let path = Path::new(&file);
        let file_stem = path.file_stem().unwrap().to_string_lossy();
        let out_file = format!("{}.{}", file_stem, output_type);
        let out_path = target_dir.join(out_file);

        let mut matrix: Vec<Vec<String>> = Vec::new();

        if input_type == "csv" || input_type == "txt" {
            let delimiter = if input_type == "csv" { b',' } else { b'\t' };
            let mut rdr = ReaderBuilder::new()
                .delimiter(delimiter)
                .has_headers(false)
                .from_path(&file)
                .map_err(|e| e.to_string())?;
                
            for result in rdr.records() {
                let record = result.map_err(|e| e.to_string())?;
                let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
                matrix.push(row);
            }
        } else if input_type == "xlsx" {
            let mut workbook = open_workbook_auto(&file).map_err(|e| e.to_string())?;
            let sheets = workbook.sheet_names().to_owned();
            if let Some(sheet_name) = sheets.first() {
                if let Ok(range) = workbook.worksheet_range(sheet_name) {
                    for row in range.rows() {
                        let mut row_vec = Vec::new();
                        for cell in row {
                            let val = match cell {
                                Data::String(s) => s.to_string(),
                                Data::Float(f) => f.to_string(),
                                Data::Int(i) => i.to_string(),
                                Data::Bool(b) => b.to_string(),
                                _ => "".to_string(),
                            };
                            row_vec.push(val);
                        }
                        matrix.push(row_vec);
                    }
                }
            }
        }

        if output_type == "csv" || output_type == "txt" {
            let delimiter = if output_type == "csv" { b',' } else { b'\t' };
            let mut wtr = WriterBuilder::new()
                .delimiter(delimiter)
                .from_path(&out_path)
                .map_err(|e| e.to_string())?;
                
            for row in matrix {
                wtr.write_record(&row).map_err(|e| e.to_string())?;
            }
            wtr.flush().map_err(|e| e.to_string())?;
            
        } else if output_type == "xlsx" {
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            for (r_idx, row) in matrix.iter().enumerate() {
                for (c_idx, val) in row.iter().enumerate() {
                    ws.write_string(r_idx as u32, c_idx as u16, val).map_err(|e| e.to_string())?;
                }
            }
            wb.save(&out_path).map_err(|e| e.to_string())?;
        }
    }
    
    app.emit("job-log", serde_json::json!({ "message": "Dönüşüm tamamlandı.", "percent": 100 })).unwrap_or(());
    Ok(target_dir.to_string_lossy().to_string())
}
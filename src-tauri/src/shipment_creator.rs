use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::Workbook;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use rustc_hash::{FxHashMap, FxHashSet};

fn get_col_idx(headers: &[String], possible_names: Option<&Vec<Value>>, context: &str) -> Result<usize, String> {
    let names = possible_names.ok_or(format!("Ayarlarda sütun listesi eksik: {}", context))?;
    for val in names {
        if let Some(name) = val.as_str() {
            if let Some(idx) = headers.iter().position(|h| h.eq_ignore_ascii_case(name)) {
                return Ok(idx);
            }
        }
    }
    Err(format!("Eksik Sütun: {} için beklenen sütunlardan hiçbiri bulunamadı.", context))
}

fn clean_upc(cell: &Data) -> String {
    let mut s = cell.to_string().trim().to_string();
    if let Some(idx) = s.find('.') {
        s.truncate(idx);
    }
    if s.len() < 12 && !s.is_empty() {
        s = format!("{:0>12}", s);
    }
    s
}

#[derive(Clone)]
struct InvoiceRow {
    ship_quantity: String,
    price: String,
    pack_size: String,
    brand: String,
    description: String,
}

#[derive(Clone)]
struct ResOrdRow {
    price_check: String,
    suplier: String,
    asin: String,
    pcs: String,
    pk: String,
    sku: String,
    dosya: String,
}

struct CombinedRow {
    upc: String,
    inv: InvoiceRow,
    ro: Option<ResOrdRow>,
    dosya_flag: String,
    
    // Calc fields
    sku2: String,
    yeni_pcs: f64,
    pk_each: f64,
    kalan: f64,
    num_pcs: f64,
    num_pk: f64,
    num_ship_qty: f64,
    has_pk: bool,
    row_idx: usize,
}

pub fn process(
    app: &AppHandle,
    cancel_flag: &AtomicBool,
    invoice_files: Vec<String>,
    orderform_files: Vec<String>,
    restock_files: Vec<String>,
    output_folder: String,
    save_name: String,
    dc_code: String,
    settings: Value,
) -> Result<String, String> {
    if invoice_files.is_empty() || orderform_files.is_empty() || restock_files.is_empty() {
        return Err("Hata: Gerekli kaynak dosyalardan biri eksik.".to_string());
    }

    let r_cols = settings.get("restock_columns").and_then(|v| v.as_object()).ok_or("restock_columns ayarı eksik.")?;
    let o_cols = settings.get("orderform_columns").and_then(|v| v.as_object()).ok_or("orderform_columns ayarı eksik.")?;
    let i_cols = settings.get("invoice_columns").and_then(|v| v.as_object()).ok_or("invoice_columns ayarı eksik.")?;

    // 1. INVOICE VERİSİ
    app.emit("job-log", serde_json::json!({ "message": "Invoice dosyası işleniyor...", "percent": 10 })).unwrap_or(());
    let mut inv_wb = open_workbook_auto(&invoice_files[0]).map_err(|e| e.to_string())?;
    let inv_sheet = inv_wb.sheet_names().first().unwrap().clone();
    let inv_range = inv_wb.worksheet_range(&inv_sheet).map_err(|e| e.to_string())?;
    let mut inv_iter = inv_range.rows();
    let inv_header = inv_iter.next().ok_or("Invoice başlığı bulunamadı.")?;
    let inv_headers: Vec<String> = inv_header.iter().map(|c| c.to_string().trim().to_string()).collect();

    let i_upc_idx = get_col_idx(&inv_headers, i_cols.get("upc").and_then(|v| v.as_array()), "Invoice UPC")?;
    let i_qty_idx = get_col_idx(&inv_headers, i_cols.get("shipquantity").and_then(|v| v.as_array()), "Invoice ShipQuantity")?;
    let i_prc_idx = get_col_idx(&inv_headers, i_cols.get("price").and_then(|v| v.as_array()), "Invoice Price")?;
    let i_psz_idx = get_col_idx(&inv_headers, i_cols.get("packsize").and_then(|v| v.as_array()), "Invoice PackSize")?;
    let i_brd_idx = get_col_idx(&inv_headers, i_cols.get("brand").and_then(|v| v.as_array()), "Invoice Brand")?;
    let i_dsc_idx = get_col_idx(&inv_headers, i_cols.get("description").and_then(|v| v.as_array()), "Invoice Description")?;

    let mut invoice_data: Vec<(String, InvoiceRow)> = Vec::with_capacity(inv_range.get_size().0);
    for row in inv_iter {
        let upc = clean_upc(row.get(i_upc_idx).unwrap_or(&Data::Empty));
        if upc.is_empty() { continue; }
        invoice_data.push((upc, InvoiceRow {
            ship_quantity: row.get(i_qty_idx).unwrap_or(&Data::Empty).to_string(),
            price: row.get(i_prc_idx).unwrap_or(&Data::Empty).to_string(),
            pack_size: row.get(i_psz_idx).unwrap_or(&Data::Empty).to_string(),
            brand: row.get(i_brd_idx).unwrap_or(&Data::Empty).to_string(),
            description: row.get(i_dsc_idx).unwrap_or(&Data::Empty).to_string(),
        }));
    }

    // 2. RESTOCK VERİSİ
    app.emit("job-log", serde_json::json!({ "message": "Restock dosyası işleniyor...", "percent": 30 })).unwrap_or(());
    let mut res_wb = open_workbook_auto(&restock_files[0]).map_err(|e| e.to_string())?;
    let res_sheet = res_wb.sheet_names().first().unwrap().clone();
    let res_range = res_wb.worksheet_range(&res_sheet).map_err(|e| e.to_string())?;
    let mut res_iter = res_range.rows();
    let res_header = res_iter.next().ok_or("Restock başlığı bulunamadı.")?;
    let res_headers: Vec<String> = res_header.iter().map(|c| c.to_string().trim().to_string()).collect();

    let r_upc_idx = get_col_idx(&res_headers, r_cols.get("upc").and_then(|v| v.as_array()), "Restock UPC")?;
    let r_prc_idx = get_col_idx(&res_headers, r_cols.get("price").and_then(|v| v.as_array()), "Restock Price")?;
    let r_sup_idx = get_col_idx(&res_headers, r_cols.get("suplier").and_then(|v| v.as_array()), "Restock Suplier")?;
    let r_asn_idx = get_col_idx(&res_headers, r_cols.get("asin").and_then(|v| v.as_array()), "Restock ASIN")?;
    let r_pcs_idx = get_col_idx(&res_headers, r_cols.get("pcs").and_then(|v| v.as_array()), "Restock PCS")?;
    let r_pk_idx = get_col_idx(&res_headers, r_cols.get("pk").and_then(|v| v.as_array()), "Restock PK")?;

    let mut restock_map: FxHashMap<String, Vec<ResOrdRow>> = FxHashMap::default();
    let mut raw_res_upcs: FxHashSet<String> = FxHashSet::default();

    for row in res_iter {
        let pcs_str = row.get(r_pcs_idx).unwrap_or(&Data::Empty).to_string().trim().to_string();
        if pcs_str.is_empty() || pcs_str.eq_ignore_ascii_case("nan") { continue; } // Sadece Pcs doluysa
        
        let upc = clean_upc(row.get(r_upc_idx).unwrap_or(&Data::Empty));
        if upc.is_empty() { continue; }

        raw_res_upcs.insert(upc.clone());
        restock_map.entry(upc).or_default().push(ResOrdRow {
            price_check: row.get(r_prc_idx).unwrap_or(&Data::Empty).to_string(),
            suplier: row.get(r_sup_idx).unwrap_or(&Data::Empty).to_string(),
            asin: row.get(r_asn_idx).unwrap_or(&Data::Empty).to_string(),
            pcs: pcs_str,
            pk: row.get(r_pk_idx).unwrap_or(&Data::Empty).to_string(),
            sku: "#YOK".to_string(),
            dosya: "Restock".to_string(),
        });
    }

    // 3. ORDER FORM VERİSİ (Melt / Unpivot İşlemi)
    app.emit("job-log", serde_json::json!({ "message": "Order Form dosyası işleniyor...", "percent": 50 })).unwrap_or(());
    let mut ord_wb = open_workbook_auto(&orderform_files[0]).map_err(|e| e.to_string())?;
    let ord_sheet = ord_wb.sheet_names().first().unwrap().clone();
    let ord_range = ord_wb.worksheet_range(&ord_sheet).map_err(|e| e.to_string())?;
    let mut ord_iter = ord_range.rows();
    let ord_header = ord_iter.next().ok_or("Order Form başlığı bulunamadı.")?;
    let ord_headers: Vec<String> = ord_header.iter().map(|c| c.to_string().trim().to_string()).collect();

    let o_upc_idx = get_col_idx(&ord_headers, o_cols.get("upc").and_then(|v| v.as_array()), "OrderForm UPC")?;
    let o_prc_idx = get_col_idx(&ord_headers, o_cols.get("price").and_then(|v| v.as_array()), "OrderForm Price")?;
    let o_sup_idx = get_col_idx(&ord_headers, o_cols.get("suplier").and_then(|v| v.as_array()), "OrderForm Suplier")?;

    let o_asin_names = o_cols.get("asin").unwrap().as_array().unwrap();
    let o_sku_names = o_cols.get("sku").unwrap().as_array().unwrap();
    let o_pcs_target = o_cols.get("pcs").unwrap().as_array().unwrap()[0].as_str().unwrap();

    let pcs_indices: Vec<usize> = ord_headers.iter().enumerate()
        .filter(|(_, h)| h.eq_ignore_ascii_case(o_pcs_target))
        .map(|(i, _)| i).collect();

    let mut raw_ord_upcs: FxHashSet<String> = FxHashSet::default();
    let mut order_map: FxHashMap<String, Vec<ResOrdRow>> = FxHashMap::default();

    for row in ord_iter {
        let upc = clean_upc(row.get(o_upc_idx).unwrap_or(&Data::Empty));
        if upc.is_empty() { continue; }
        raw_ord_upcs.insert(upc.clone());

        for i in 0..o_asin_names.len() {
            if cancel_flag.load(Ordering::Relaxed) { return Err("İşlem iptal edildi.".to_string()); }
            let asin_name = o_asin_names[i].as_str().unwrap();
            let sku_name = o_sku_names[i].as_str().unwrap();

            let asin_idx = ord_headers.iter().position(|h| h.eq_ignore_ascii_case(asin_name));
            let sku_idx = ord_headers.iter().position(|h| h.eq_ignore_ascii_case(sku_name));
            let pcs_idx = pcs_indices.get(i).copied();

            if asin_idx.is_none() || sku_idx.is_none() || pcs_idx.is_none() { break; }

            let asin_str = row.get(asin_idx.unwrap()).unwrap_or(&Data::Empty).to_string().trim().to_string();
            if asin_str.is_empty() || asin_str.eq_ignore_ascii_case("nan") { continue; }

            let sku_str = row.get(sku_idx.unwrap()).unwrap_or(&Data::Empty).to_string().trim().to_string();
            let pk = if sku_str.chars().filter(|&c| c == '_').count() >= 3 {
                sku_str.split('_').nth(2).unwrap_or("#YOK").to_string()
            } else {
                "#YOK".to_string()
            };

            order_map.entry(upc.clone()).or_default().push(ResOrdRow {
                price_check: row.get(o_prc_idx).unwrap_or(&Data::Empty).to_string(),
                suplier: row.get(o_sup_idx).unwrap_or(&Data::Empty).to_string(),
                asin: asin_str,
                pcs: row.get(pcs_idx.unwrap()).unwrap_or(&Data::Empty).to_string(),
                pk,
                sku: if sku_str.is_empty() { "#YOK".to_string() } else { sku_str },
                dosya: "Order Form".to_string(),
            });
        }
    }

    // 4. EŞLEŞTİRME VE DÜZENLEME (INNER / LEFT JOIN)
    app.emit("job-log", serde_json::json!({ "message": "O(1) Hızında İlişkisel Eşleşme (Join) yapılıyor...", "percent": 70 })).unwrap_or(());
    let mut combined_rows: Vec<CombinedRow> = Vec::new();
    let mut row_counter = 0;

    for (upc, inv_row) in invoice_data {
        let res_matches = restock_map.get(&upc);
        let ord_matches = order_map.get(&upc);

        let mut matched_res = false;
        let mut matched_ord = false;

        let num_ship_qty = inv_row.ship_quantity.parse::<f64>().unwrap_or(0.0);

        let mut push_row = |ro: Option<&ResOrdRow>, dosya_flag: &str| {
            let (num_pcs, pk_str) = if let Some(r) = ro {
                (r.pcs.replace(",", ".").parse::<f64>().unwrap_or(0.0), r.pk.clone())
            } else { (0.0, "#YOK".to_string()) };

            let num_pk = pk_str.replace("PK", "").parse::<f64>().unwrap_or(0.0);
            let has_pk = pk_str != "#YOK" && num_pk > 0.0;

            combined_rows.push(CombinedRow {
                upc: upc.clone(),
                inv: inv_row.clone(),
                ro: ro.cloned(),
                dosya_flag: dosya_flag.to_string(),
                sku2: "#YOK".to_string(),
                yeni_pcs: 0.0,
                pk_each: 0.0,
                kalan: 0.0,
                num_pcs,
                num_pk,
                num_ship_qty,
                has_pk,
                row_idx: row_counter,
            });
            row_counter += 1;
        };

        if let Some(rm) = res_matches {
            for r in rm { push_row(Some(r), "Restock"); matched_res = true; }
        }
        if let Some(om) = ord_matches {
            for o in om { push_row(Some(o), "Order Form"); matched_ord = true; }
        }

        if !matched_res && raw_res_upcs.contains(&upc) { push_row(None, "Restock"); }
        if !matched_ord && raw_ord_upcs.contains(&upc) { push_row(None, "Order Form"); }
        if !raw_res_upcs.contains(&upc) && !raw_ord_upcs.contains(&upc) { push_row(None, "#YOK"); }
    }

    // SKU2 ÇOĞULLAMA VE HESAPLAMA
    let mut sku2_counts: FxHashMap<String, usize> = FxHashMap::default();
    let letters = ["", "_A", "_B", "_C", "_D", "_E", "_F", "_G", "_H", "_I", "_J"];

    for row in combined_rows.iter_mut() {
        if let Some(ro) = &row.ro {
            if ro.pk != "#YOK" && row.inv.price != "#YOK" && !row.inv.price.is_empty() {
                let price_f = row.inv.price.replace(",", ".").parse::<f64>().unwrap_or(0.0);
                let calc_val = row.num_pk * price_f;
                let base_sku2 = format!("{}_{}_{}_{:.2}", dc_code, row.upc, ro.pk, calc_val);
                
                let count = sku2_counts.entry(base_sku2.clone()).or_insert(0);
                let suffix = if *count < letters.len() { letters[*count] } else { &format!("_{}", count) };
                row.sku2 = format!("{}{}", base_sku2, suffix);
                *count += 1;
            }
        }
    }

    // 5. STOK DAĞITIMI (ALLOCATOR - Window Functions)
    app.emit("job-log", serde_json::json!({ "message": "Stoklar vektörel olarak dağıtılıyor...", "percent": 85 })).unwrap_or(());
    
    let mut upc_groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (i, row) in combined_rows.iter().enumerate() {
        upc_groups.entry(row.upc.clone()).or_default().push(i);
    }

    for (_, indices) in upc_groups.iter() {
        let mut total_pcs = 0.0;
        let mut total_kalan = 0.0;
        let mut best_idx_for_remainder: Option<usize> = None;
        let mut min_num_pk = f64::MAX;
        let mut best_row_idx = 0;

        for &i in indices { total_pcs += combined_rows[i].num_pcs; }

        for &i in indices {
            let row = &mut combined_rows[i];
            let mut base_new_pcs = 0.0;
            if total_pcs > 0.0 {
                base_new_pcs = ((row.num_pcs / total_pcs) * row.num_ship_qty).round();
            }

            let mut kalan = 0.0;
            if row.has_pk && row.num_pk > 0.0 {
                kalan = base_new_pcs % row.num_pk;
                row.yeni_pcs = base_new_pcs - kalan;
            } else {
                row.yeni_pcs = base_new_pcs;
            }

            total_kalan += kalan;

            if row.has_pk && row.num_pk > 0.0 {
                if row.num_pk < min_num_pk || (row.num_pk == min_num_pk && row.row_idx > best_row_idx) {
                    min_num_pk = row.num_pk;
                    best_row_idx = row.row_idx;
                    best_idx_for_remainder = Some(i);
                }
            }
        }

        if let Some(best_i) = best_idx_for_remainder {
            combined_rows[best_i].yeni_pcs += total_kalan;
        }

        for &i in indices {
            let row = &mut combined_rows[i];
            if row.has_pk && row.num_pk > 0.0 {
                row.pk_each = (row.yeni_pcs / row.num_pk).floor();
                row.kalan = row.yeni_pcs % row.num_pk;
            }
        }
    }

// 6. EXCEL'E KAYIT
    app.emit("job-log", serde_json::json!({ "message": "Sonuç Excel dosyasına kaydediliyor...", "percent": 95 })).unwrap_or(());
    std::fs::create_dir_all(&output_folder).map_err(|e| e.to_string())?;
    let out_path = Path::new(&output_folder).join(format!("{}.xlsx", save_name));

    let mut wb_out = Workbook::new();
    let ws = wb_out.add_worksheet();

    let headers = ["UPC", "Price", "Price Check", "Suplier", "ShipQuantity", "Asin", "Pcs", "Yeni Pcs", "PK", "SKU", "PackSize", "Brand", "Description", "DOSYA", "SKU2", "PK EACH", "Kalan"];
    
    for (c, h) in headers.iter().enumerate() { ws.write_string(0, c as u16, *h).map_err(|e| e.to_string())?; }

    // ÇÖZÜM: Closure yerine Lifetime garantili lokal fonksiyon kullanıldı.
    fn fallback(s: &str) -> &str {
        if s.is_empty() { "#YOK" } else { s }
    }

    for (r_idx, row) in combined_rows.iter().enumerate() {
        let r = (r_idx + 1) as u32;
        ws.write_string(r, 0, fallback(&row.upc)).map_err(|e| e.to_string())?;
        ws.write_string(r, 1, fallback(&row.inv.price)).map_err(|e| e.to_string())?;
        
        let p_chk = row.ro.as_ref().map(|x| x.price_check.as_str()).unwrap_or("");
        ws.write_string(r, 2, fallback(p_chk)).map_err(|e| e.to_string())?;
        
        let sup = row.ro.as_ref().map(|x| x.suplier.as_str()).unwrap_or("");
        ws.write_string(r, 3, fallback(sup)).map_err(|e| e.to_string())?;
        
        ws.write_string(r, 4, fallback(&row.inv.ship_quantity)).map_err(|e| e.to_string())?;
        
        let asn = row.ro.as_ref().map(|x| x.asin.as_str()).unwrap_or("");
        ws.write_string(r, 5, fallback(asn)).map_err(|e| e.to_string())?;
        
        let pcs = row.ro.as_ref().map(|x| x.pcs.as_str()).unwrap_or("");
        ws.write_string(r, 6, fallback(pcs)).map_err(|e| e.to_string())?;
        
        ws.write_number(r, 7, row.yeni_pcs).map_err(|e| e.to_string())?;
        
        let pk = row.ro.as_ref().map(|x| x.pk.as_str()).unwrap_or("");
        ws.write_string(r, 8, fallback(pk)).map_err(|e| e.to_string())?;
        
        let sku = row.ro.as_ref().map(|x| x.sku.as_str()).unwrap_or("");
        ws.write_string(r, 9, fallback(sku)).map_err(|e| e.to_string())?;
        
        ws.write_string(r, 10, fallback(&row.inv.pack_size)).map_err(|e| e.to_string())?;
        ws.write_string(r, 11, fallback(&row.inv.brand)).map_err(|e| e.to_string())?;
        ws.write_string(r, 12, fallback(&row.inv.description)).map_err(|e| e.to_string())?;
        ws.write_string(r, 13, fallback(&row.dosya_flag)).map_err(|e| e.to_string())?;
        ws.write_string(r, 14, fallback(&row.sku2)).map_err(|e| e.to_string())?;
        
        ws.write_number(r, 15, row.pk_each).map_err(|e| e.to_string())?;
        ws.write_number(r, 16, row.kalan).map_err(|e| e.to_string())?;
    }

    wb_out.save(&out_path).map_err(|e| e.to_string())?;
    app.emit("job-log", serde_json::json!({ "message": "İşlem tamamlandı!", "percent": 100 })).unwrap_or(());
    Ok(output_folder)
}
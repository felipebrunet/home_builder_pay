use std::path::PathBuf;

use konstruado_core::{
    monto, titulo_partida, EstadoObra, Obra, PartidaEstado,
};

use crate::fmt_cuando;

pub fn constancia(obra: &Obra) -> String {
    let mut s = String::new();
    s.push_str("KONSTRUADO — constancia de obra\n");
    s.push_str("================================\n\n");
    s.push_str(&obra.nombre);
    s.push('\n');
    s.push_str(&format!("Estado: {}\n\n", estado(&obra.estado)));
    s.push_str(&format!("Mandante: {}\n", obra.mandante.nombre));
    s.push_str(&format!("Contratista: {}\n\n", obra.contratista.nombre));
    s.push_str(&format!("Trabajo: {}\n", monto(obra.trabajo)));
    s.push_str(&format!("Garantía por partida: {}\n", monto(obra.garantia)));
    s.push_str(&format!("Partidas: {}\n", obra.n_partidas));
    if let Some(ex) = obra.extra.as_ref() {
        s.push_str(&format!(
            "Partida extra propuesta por {}: {}\n",
            ex.por.nombre, ex.detalle
        ));
    }
    for (i, p) in obra.partidas.iter().enumerate() {
        let titulo = titulo_partida(i, &p.detalle);
        s.push_str(&format!(
            "\n--- Partida {} · {} · {} ---\n",
            i + 1,
            titulo,
            partida_estado(p.estado)
        ));
        if let Some(q) = p.encerrado_por.as_ref() {
            s.push_str(&format!(
                "Encerró {} · {}\n",
                q.nombre,
                fmt_cuando(p.encerrado_cuando)
            ));
        }
        if let Some(t) = p.turno {
            s.push_str(&format!("Turno: {}\n", t.etiqueta()));
        }
        if !p.notas.is_empty() {
            s.push_str("Notas:\n");
            for n in &p.notas {
                s.push_str(&format!(
                    "  {}  {} · {}%\n",
                    fmt_cuando(n.cuando),
                    n.autor_nombre,
                    n.porcentaje
                ));
                if !n.texto.is_empty() {
                    s.push_str(&format!("    {}\n", n.texto));
                }
            }
        }
        if let Some(r) = p.recibo.as_ref() {
            s.push_str(&format!(
                "Recibo: {}\n  Pagó {}% · {} · aceptó {} · {}\n",
                r.titulo,
                r.porcentaje,
                monto(r.monto),
                r.acepto_nombre,
                fmt_cuando(r.cuando)
            ));
        }
    }
    s.push('\n');
    s
}

pub fn guardar_txt(obra: &Obra) -> Result<PathBuf, String> {
    let suggested = format!("konstruado-{}.txt", slug(&obra.nombre));
    let texto = constancia(obra);
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Exportar constancia")
        .set_file_name(&suggested)
        .add_filter("Texto", &["txt"])
        .save_file()
    {
        std::fs::write(&path, texto.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(path);
    }
    Err("No se eligió dónde guardar.".into())
}

pub fn guardar_pdf(obra: &Obra) -> Result<PathBuf, String> {
    let suggested = format!("konstruado-{}.pdf", slug(&obra.nombre));
    let Some(path) = rfd::FileDialog::new()
        .set_title("Exportar PDF")
        .set_file_name(&suggested)
        .add_filter("PDF", &["pdf"])
        .save_file()
    else {
        return Err("No se eligió dónde guardar.".into());
    };
    let bytes = pdf_bytes(obra)?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn pdf_bytes(obra: &Obra) -> Result<Vec<u8>, String> {
    use printpdf::*;
    use std::io::Cursor;

    let (doc, page1, layer1) = PdfDocument::new("Konstruado", Mm(210.0), Mm(297.0), "Capa");
    let font = pdf_font(&doc)?;
    let lines = wrap_lines(&constancia(obra), 92);
    let mut pages: Vec<(PdfPageIndex, PdfLayerIndex)> = vec![(page1, layer1)];
    let mut page_i = 0usize;
    let mut y = 280.0;
    for line in lines {
        if y < 18.0 {
            let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Capa");
            pages.push((p, l));
            page_i += 1;
            y = 280.0;
        }
        let (p, l) = pages[page_i];
        let layer = doc.get_page(p).get_layer(l);
        layer.use_text(line, 10.0, Mm(18.0), Mm(y), &font);
        y -= 5.0;
    }
    let mut buf = std::io::BufWriter::new(Cursor::new(Vec::new()));
    doc.save(&mut buf).map_err(|e| e.to_string())?;
    let cur = buf.into_inner().map_err(|e| e.to_string())?;
    Ok(cur.into_inner())
}

fn pdf_font(doc: &printpdf::PdfDocumentReference) -> Result<printpdf::IndirectFontRef, String> {
    use printpdf::*;
    for p in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
    ] {
        if let Ok(f) = std::fs::File::open(p) {
            if let Ok(font) = doc.add_external_font(f) {
                return Ok(font);
            }
        }
    }
    doc.add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| e.to_string())
}

fn wrap_lines(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in s.lines() {
        if raw.chars().count() <= width {
            out.push(raw.to_string());
            continue;
        }
        let mut cur = String::new();
        for w in raw.split_whitespace() {
            if cur.is_empty() {
                cur = w.to_string();
            } else if cur.chars().count() + 1 + w.chars().count() <= width {
                cur.push(' ');
                cur.push_str(w);
            } else {
                out.push(std::mem::take(&mut cur));
                cur = w.to_string();
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn estado(e: &EstadoObra) -> &'static str {
    match e {
        EstadoObra::Publicada => "Publicada",
        EstadoObra::Contra => "Contra",
        EstadoObra::Rechazada => "Rechazada",
        EstadoObra::Acordada => "Acordada",
        EstadoObra::EnMarcha => "En marcha",
        EstadoObra::Abandonada => "Abandonada",
        EstadoObra::Cerrada => "Cerrada",
    }
}

fn partida_estado(e: PartidaEstado) -> &'static str {
    match e {
        PartidaEstado::Pendiente => "Pendiente",
        PartidaEstado::Encerrada => "Encerrada",
        PartidaEstado::EnTrato => "En trato",
        PartidaEstado::Pagada => "Pagada",
    }
}

fn slug(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let t = t.trim_matches('-');
    if t.is_empty() { "obra".into() } else { t.into() }
}

#[cfg(test)]
mod tests {
    use konstruado_core::{Aceptacion, Oferta, Persona};

    use super::*;

    #[test]
    fn constancia_lleva_nombres() {
        let m = Persona::nueva("Don Dinero").unwrap();
        let c = Persona::nueva("Don Chasquilla").unwrap();
        let o = Oferta::publicar(m, "Casa El Quisco", 10_000, 5_000, vec!["Fundaciones".into()])
            .unwrap();
        let a = Aceptacion::de(&o, c, 5_000).unwrap();
        let obra = konstruado_core::Obra::desde_oferta(o, a).unwrap();
        let t = constancia(&obra);
        assert!(t.contains("Casa El Quisco"));
        assert!(t.contains("Don Dinero"));
        assert!(t.contains("Don Chasquilla"));
        assert!(t.contains("Fundaciones"));
        assert!(t.contains("2"));
    }
}

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

pub fn guardar(obra: &Obra) -> Result<PathBuf, String> {
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

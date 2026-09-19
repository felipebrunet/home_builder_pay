use konstruado_core::{Error, EstadoObra, Partida, PartidaEstado, Rol};
use konstruado_net::EstadoTor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idioma {
    Es,
    En,
}

impl Idioma {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("en") {
            Self::En
        } else {
            Self::Es
        }
    }

    pub fn codigo(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }

    pub fn t(self, es: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Es => es,
            Self::En => en,
        }
    }

    pub fn rol(self, r: Rol) -> &'static str {
        match (self, r) {
            (Self::Es, Rol::Mandante) => "mandante",
            (Self::Es, Rol::Contratista) => "contratista",
            (Self::En, Rol::Mandante) => "client",
            (Self::En, Rol::Contratista) => "contractor",
        }
    }

    pub fn label_estado(self, e: EstadoObra) -> &'static str {
        match (self, e) {
            (Self::Es, EstadoObra::Publicada) => "Publicada",
            (Self::Es, EstadoObra::Contra) => "Contra",
            (Self::Es, EstadoObra::Rechazada) => "Rechazada",
            (Self::Es, EstadoObra::Acordada) => "Acordada",
            (Self::Es, EstadoObra::EnMarcha) => "En marcha",
            (Self::Es, EstadoObra::Abandonada) => "Abandonada",
            (Self::Es, EstadoObra::Cerrada) => "Cerrada",
            (Self::En, EstadoObra::Publicada) => "Posted",
            (Self::En, EstadoObra::Contra) => "Counter",
            (Self::En, EstadoObra::Rechazada) => "Rejected",
            (Self::En, EstadoObra::Acordada) => "Agreed",
            (Self::En, EstadoObra::EnMarcha) => "Underway",
            (Self::En, EstadoObra::Abandonada) => "Abandoned",
            (Self::En, EstadoObra::Cerrada) => "Closed",
        }
    }

    pub fn label_partida(self, p: &Partida) -> String {
        match p.estado {
            PartidaEstado::Pendiente => self.t("Pendiente", "Pending").into(),
            PartidaEstado::Encerrando => self.t("Encerrando", "Locking").into(),
            PartidaEstado::Encerrada => self.t("En obra", "In progress").into(),
            PartidaEstado::EnTrato => match p.propuesto {
                Some(n) => match self {
                    Self::Es => format!("Trato {n}%"),
                    Self::En => format!("Deal {n}%"),
                },
                None => self.t("En trato", "In deal").into(),
            },
            PartidaEstado::Pagada => match p.pago {
                Some(n) => match self {
                    Self::Es => format!("Pagada {n}%"),
                    Self::En => format!("Paid {n}%"),
                },
                None => self.t("Pagada", "Paid").into(),
            },
        }
    }

    pub fn titulo_partida(self, i: usize, detalle: &str) -> String {
        if !detalle.trim().is_empty() {
            return detalle.to_string();
        }
        match self {
            Self::Es => format!("Partida {}", i + 1),
            Self::En => format!("Stage {}", i + 1),
        }
    }

    pub fn placeholder_partida(self, i: usize, n: u32) -> &'static str {
        const ES: [&str; 5] = [
            "Cimientos",
            "Muros",
            "Techumbre",
            "Instalaciones",
            "Terminaciones",
        ];
        const EN: [&str; 5] = [
            "Foundations",
            "Walls",
            "Roof",
            "Installations",
            "Finishes",
        ];
        if n == 5 && i < 5 {
            match self {
                Self::Es => ES[i],
                Self::En => EN[i],
            }
        } else {
            self.t("Qué se hace en esta partida", "What this stage covers")
        }
    }

    pub fn error(self, e: &Error) -> String {
        match (self, e) {
            (Self::Es, _) => e.to_string(),
            (Self::En, Error::Nombre) => "Enter a name.".into(),
            (Self::En, Error::Monto) => "The job amount has to be greater than zero.".into(),
            (Self::En, Error::Garantia) => "The guarantee has to be greater than zero.".into(),
            (Self::En, Error::NoDivide) => {
                "The guarantee has to divide the job amount exactly (equal stages).".into()
            }
            (Self::En, Error::NoEsta) => "I can't find that.".into(),
            (Self::En, Error::NoToca) => "That is not your turn.".into(),
            (Self::En, Error::YaExiste) => "That is already done.".into(),
            (Self::En, Error::Nota) => "The note cannot be longer than 50 characters.".into(),
            (Self::En, Error::Porcentaje) => "Payment is from 1 to 100 percent.".into(),
            (Self::En, Error::Detalle) => "Write what the extra stage is.".into(),
            (Self::En, Error::Desconectado) => {
                "The other person is offline. They need Konstruado open.".into()
            }
        }
    }

    pub fn fmt_cuando(self, ts: i64) -> String {
        if ts <= 0 {
            return String::new();
        }
        let fmt = match self {
            Self::Es => "%d/%m/%Y %H:%M",
            Self::En => "%Y-%m-%d %H:%M",
        };
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|d| d.format(fmt).to_string())
            .unwrap_or_default()
    }

    pub fn paso_tor(self, paso: &str) -> String {
        if self == Self::Es {
            return paso.to_string();
        }
        if let Some(rest) = paso.strip_prefix("publicando sala (") {
            return format!("publishing room ({rest}");
        }
        if let Some(rest) = paso.strip_prefix("buscando sala (") {
            let inner = rest.trim_end_matches(')');
            let inner = match inner {
                "sin respuesta" => "no reply",
                "sala aún no visible" => "room not visible yet",
                other => other,
            };
            return format!("looking for room ({inner})");
        }
        if let Some(rest) = paso.strip_prefix("bootstrap trabado en ") {
            return format!("bootstrap stuck at {rest}");
        }
        if let Some(rest) = paso.strip_prefix("bootstrap lento (") {
            return format!("bootstrap slow ({rest}");
        }
        match paso {
            "buscando tor" => "looking for tor".into(),
            "lanzando tor" => "starting tor".into(),
            "control" => "control".into(),
            "onion personal" => "personal onion".into(),
            "tor…" => "tor…".into(),
            "tor listo, esperá a entrar" => "tor ready, wait to join".into(),
            "abriendo sala" => "opening room".into(),
            "sala abierta, esperando al contratista" => {
                "room open, waiting for contractor".into()
            }
            "sala aún no visible" => "room not visible yet".into(),
            other => other.to_string(),
        }
    }

    pub fn linea_red(self, tor: EstadoTor, peers: usize, otros: &[String]) -> String {
        let tor_txt = match tor {
            EstadoTor::Listo { onion } => {
                let corto = onion.get(..8).unwrap_or(onion.as_str());
                format!("Tor {corto}…")
            }
            EstadoTor::Arrancando { paso } => format!("Tor {}", self.paso_tor(&paso)),
            EstadoTor::Fallo(s) => format!("Tor: {}", self.paso_tor(&s)),
            EstadoTor::Ausente => self.t("Red local", "Local net").into(),
        };
        let gente = if otros.is_empty() {
            if peers == 0 {
                self.t("nadie más en la red", "nobody else on the net")
                    .to_string()
            } else {
                match self {
                    Self::Es => format!("{peers} par(es), todavía sin nombre"),
                    Self::En => format!("{peers} peer(s), still unnamed"),
                }
            }
        } else {
            otros.join(", ")
        };
        format!("{tor_txt} · {gente}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingles_cambia_etiquetas() {
        let en = Idioma::En;
        assert_eq!(en.t("Mis obras", "My jobs"), "My jobs");
        assert_eq!(en.rol(Rol::Mandante), "client");
        assert_eq!(en.label_estado(EstadoObra::EnMarcha), "Underway");
        assert_eq!(en.titulo_partida(0, ""), "Stage 1");
        assert_eq!(en.titulo_partida(0, "Foundations"), "Foundations");
        assert_eq!(
            en.error(&Error::Nombre),
            "Enter a name."
        );
        assert_eq!(Idioma::Es.t("Mis obras", "My jobs"), "Mis obras");
        assert_eq!(Idioma::parse(""), Idioma::Es);
        assert_eq!(Idioma::parse("en").codigo(), "en");
    }
}

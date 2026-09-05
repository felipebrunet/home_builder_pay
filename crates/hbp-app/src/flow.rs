//! One next action for the current obra / trato. Step-1 UI map.

use hbp_core::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextKind {
    Prepare,
    Sign,
    Connect,
    Publish,
    Send,
    Accept,
    Search,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextStep {
    pub sentence: String,
    pub button: Option<&'static str>,
    pub kind: NextKind,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkProgress {
    pub has_draft: bool,
    pub has_offer: bool,
    pub has_pending: bool,
    pub has_signed: bool,
    pub net_up: bool,
    pub has_peer: bool,
}

pub fn next_step(role: Role, p: WorkProgress) -> NextStep {
    match role {
        Role::Mandante => mandante_step(p),
        Role::Contratista => contratista_step(p),
    }
}

fn mandante_step(p: WorkProgress) -> NextStep {
    if p.has_signed {
        return NextStep {
            sentence: "Trato cerrado. Las dos partes ya firmaron. El pago en cadena viene después."
                .into(),
            button: None,
            kind: NextKind::None,
        };
    }
    if !p.has_draft {
        return NextStep {
            sentence: "Pon el total, la moneda y los plazos, y prepara las partidas.".into(),
            button: Some("Preparar partidas"),
            kind: NextKind::Prepare,
        };
    }
    if !p.has_offer {
        return NextStep {
            sentence: "Firma la propuesta para poder enviársela al maestro.".into(),
            button: Some("Firmar propuesta"),
            kind: NextKind::Sign,
        };
    }
    if !p.net_up {
        return NextStep {
            sentence: "Conéctate para enviar la propuesta al contratista.".into(),
            button: Some("Conectarme"),
            kind: NextKind::Connect,
        };
    }
    if !p.has_peer {
        return NextStep {
            sentence:
                "Publica la obra para que el maestro te encuentre. Después envías la propuesta."
                    .into(),
            button: Some("Publicar obra"),
            kind: NextKind::Publish,
        };
    }
    NextStep {
        sentence: "Envía la propuesta al contratista.".into(),
        button: Some("Enviar"),
        kind: NextKind::Send,
    }
}

fn contratista_step(p: WorkProgress) -> NextStep {
    if p.has_signed {
        return NextStep {
            sentence: "Trato cerrado. El mandante ya confirmó. El pago en cadena viene después."
                .into(),
            button: None,
            kind: NextKind::None,
        };
    }
    if p.has_pending {
        return NextStep {
            sentence: "Ya aceptaste. Espera que el mandante confirme el trato.".into(),
            button: None,
            kind: NextKind::None,
        };
    }
    if p.has_offer {
        return NextStep {
            sentence: "Llegó la propuesta. Acéptala si estás de acuerdo.".into(),
            button: Some("Aceptar"),
            kind: NextKind::Accept,
        };
    }
    if !p.net_up {
        return NextStep {
            sentence: "Conéctate para buscar al mandante.".into(),
            button: Some("Conectarme"),
            kind: NextKind::Connect,
        };
    }
    if !p.has_peer {
        return NextStep {
            sentence: "Busca al mandante por su nombre (como lo conoces: Don José).".into(),
            button: Some("Buscar"),
            kind: NextKind::Search,
        };
    }
    NextStep {
        sentence: "Espera la propuesta del mandante. Cuando llegue, aparece el botón Aceptar."
            .into(),
        button: None,
        kind: NextKind::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mandante_signed_proposal_says_send() {
        let step = next_step(
            Role::Mandante,
            WorkProgress {
                has_draft: true,
                has_offer: true,
                net_up: true,
                has_peer: true,
                ..WorkProgress::default()
            },
        );
        assert_eq!(step.kind, NextKind::Send);
        assert_eq!(step.button, Some("Enviar"));
        assert!(step.sentence.contains("Envía la propuesta"));
    }

    #[test]
    fn contratista_after_find_waits_then_accepts() {
        let wait = next_step(
            Role::Contratista,
            WorkProgress {
                net_up: true,
                has_peer: true,
                ..WorkProgress::default()
            },
        );
        assert_eq!(wait.kind, NextKind::None);
        assert!(wait.sentence.contains("Espera la propuesta"));
        let acc = next_step(
            Role::Contratista,
            WorkProgress {
                net_up: true,
                has_peer: true,
                has_offer: true,
                has_draft: true,
                ..WorkProgress::default()
            },
        );
        assert_eq!(acc.kind, NextKind::Accept);
        assert_eq!(acc.button, Some("Aceptar"));
    }

    #[test]
    fn empty_homes_point_at_one_cta() {
        assert_eq!(
            next_step(Role::Mandante, WorkProgress::default()).kind,
            NextKind::Prepare
        );
        assert_eq!(
            next_step(Role::Contratista, WorkProgress::default()).kind,
            NextKind::Connect
        );
    }
}

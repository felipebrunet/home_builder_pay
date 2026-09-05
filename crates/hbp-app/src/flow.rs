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
    /// Mandante already published this obra. Stops the green card on Publicar.
    pub published: bool,
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
        if !p.published {
            return NextStep {
                sentence:
                    "Publica la obra para que el maestro te encuentre. Después envías la propuesta."
                        .into(),
                button: Some("Publicar obra"),
                kind: NextKind::Publish,
            };
        }
        return NextStep {
            sentence: "Obra en el catálogo. Espera que el contratista la pida. No se envía sola."
                .into(),
            button: None,
            kind: NextKind::None,
        };
    }
    NextStep {
        sentence: "El contratista pidió esta obra. Envíale la propuesta (o reenvíala).".into(),
        button: Some("Enviar"),
        kind: NextKind::Send,
    }
}

/// Finished steps, for a muted checklist above the green card. Current step is not listed.
pub fn completed_steps(role: Role, p: WorkProgress) -> Vec<&'static str> {
    match role {
        Role::Mandante => {
            let mut v = Vec::new();
            if p.has_draft {
                v.push("Preparar partidas");
            }
            if p.has_offer {
                v.push("Firmar propuesta");
            }
            if p.net_up {
                v.push("Conectarme");
            }
            if p.published || p.has_peer {
                v.push("Publicar obra");
            }
            if p.has_signed {
                v.push("Enviar");
            }
            v
        }
        Role::Contratista => {
            let mut v = Vec::new();
            if p.net_up {
                v.push("Conectarme");
            }
            if p.has_peer {
                v.push("Buscar");
            }
            if p.has_pending || p.has_signed {
                v.push("Aceptar");
            }
            v
        }
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
            sentence: "Revisa total, plazos y partidas abajo. Si estás de acuerdo, acepta.".into(),
            button: None,
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
        sentence: "Pediste esta obra. Espera la propuesta para revisarla. Aún no aceptes.".into(),
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
        assert!(step.sentence.contains("pidió") || step.sentence.contains("Envía"));
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
        assert!(wait.sentence.contains("Pediste") || wait.sentence.contains("Espera"));
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
        assert!(acc.button.is_none());
        assert!(acc.sentence.contains("Revisa"));
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

    #[test]
    fn published_without_peer_waits_not_publish() {
        let wait = next_step(
            Role::Mandante,
            WorkProgress {
                has_draft: true,
                has_offer: true,
                net_up: true,
                published: true,
                has_peer: false,
                ..WorkProgress::default()
            },
        );
        assert_eq!(wait.kind, NextKind::None);
        assert!(wait.button.is_none());
        assert!(wait.sentence.contains("catálogo") || wait.sentence.contains("pida"));
        let done = completed_steps(
            Role::Mandante,
            WorkProgress {
                has_draft: true,
                has_offer: true,
                net_up: true,
                published: true,
                ..WorkProgress::default()
            },
        );
        assert!(done.contains(&"Publicar obra"));
        assert!(!done.contains(&"Enviar"));
    }

    #[test]
    fn published_with_peer_says_send() {
        let step = next_step(
            Role::Mandante,
            WorkProgress {
                has_draft: true,
                has_offer: true,
                net_up: true,
                published: true,
                has_peer: true,
                ..WorkProgress::default()
            },
        );
        assert_eq!(step.kind, NextKind::Send);
        assert_eq!(step.button, Some("Enviar"));
    }
}

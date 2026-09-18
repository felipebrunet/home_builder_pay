use super::*;

#[test]
fn diez_mil_y_dos_mil_son_cinco() {
    assert_eq!(n_partidas(10_000, 2_000).unwrap(), 5);
    assert_eq!(capital_por_lado(2_000), 2_000);
}

#[test]
fn diez_mil_y_mil_son_diez() {
    assert_eq!(n_partidas(10_000, 1_000).unwrap(), 10);
}

#[test]
fn no_divide() {
    assert_eq!(n_partidas(10_000, 3_000), Err(Error::NoDivide));
}

#[test]
fn aceptar_igual_acuerda() {
    let m = Persona::nueva("Felipe").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec![]).unwrap();
    assert_eq!(o.n_partidas_sugeridas, 5);
    let a = Aceptacion::de(&o, c, 2_000).unwrap();
    assert!(!a.es_contra(&o));
    let obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.estado, EstadoObra::Acordada);
    assert_eq!(obra.n_partidas, 5);
    assert_eq!(obra.partidas.len(), 5);
}

#[test]
fn contra_garantia_mil() {
    let m = Persona::nueva("Felipe").unwrap();
    let mid = m.id.clone();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec![]).unwrap();
    let a = Aceptacion::de(&o, c, 1_000).unwrap();
    assert!(a.es_contra(&o));
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.estado, EstadoObra::Contra);
    obra.confirmar_contra(&mid).unwrap();
    assert_eq!(obra.n_partidas, 10);
    assert_eq!(obra.garantia, 1_000);
    assert_eq!(obra.estado, EstadoObra::Acordada);
}

#[test]
fn encerrar_y_pagar_usa_stub_xmr() {
    let m = Persona::nueva("Felipe").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m.clone(), "Muro", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Encerrada);
    obra.avisar_termino(0, &c, 100, "Listo").unwrap();
    obra.aceptar_pago(0, &m).unwrap();
    obra.encerrar_partida(1).unwrap();
    obra.avisar_termino(1, &c, 100, "").unwrap();
    obra.aceptar_pago(1, &m).unwrap();
    assert_eq!(obra.estado, EstadoObra::Cerrada);
    assert_eq!(obra.partidas[0].pago, Some(100));
}

#[test]
fn partidas_llevan_detalle() {
    let m = Persona::nueva("José").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(
        m,
        "Casa",
        10_000,
        2_000,
        vec![
            "Cimientos".into(),
            "Muros".into(),
            "Techumbre".into(),
            "Instalaciones".into(),
            "Terminaciones".into(),
        ],
    )
    .unwrap();
    assert_eq!(o.detalles[2], "Techumbre");
    let a = Aceptacion::de(&o, c, 2_000).unwrap();
    let obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.partidas[2].detalle, "Techumbre");
    assert_eq!(titulo_partida(2, &obra.partidas[2].detalle), "Techumbre");
}

#[test]
fn contra_completa_detalles_vacios() {
    let m = Persona::nueva("José").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(
        m,
        "Casa",
        10_000,
        2_000,
        vec!["Cimientos".into(), "Muros".into()],
    )
    .unwrap();
    assert_eq!(o.detalles.len(), 5);
    let a = Aceptacion::de(&o, c, 1_000).unwrap();
    assert_eq!(a.n_partidas, 10);
    assert_eq!(a.detalles.len(), 10);
    assert_eq!(a.detalles[0], "Cimientos");
    assert_eq!(a.detalles[9], "");
}

#[test]
fn partida_se_paga_al_ochenta_con_notas_congeladas() {
    let m = Persona::nueva("Don Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec!["Fundaciones".into()]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0).unwrap();
    obra.avisar_termino(0, &c, 100, "Terminé las fundaciones")
        .unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::EnTrato);
    assert_eq!(obra.partidas[0].turno, Some(Rol::Mandante));
    obra.contra_pago(0, &m, 80, "Falta la entrada de auto")
        .unwrap();
    assert_eq!(obra.partidas[0].propuesto, Some(80));
    assert_eq!(obra.partidas[0].turno, Some(Rol::Contratista));
    obra.aceptar_pago(0, &c).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Pagada);
    assert_eq!(obra.partidas[0].pago, Some(80));
    assert_eq!(obra.partidas[0].notas.len(), 2);
    assert_eq!(
        obra.avisar_termino(0, &c, 100, "otra"),
        Err(Error::NoToca)
    );
    assert_eq!(
        obra.contra_pago(0, &m, 70, "menos"),
        Err(Error::NoToca)
    );
}

#[test]
fn nota_de_mas_de_cincuenta_no_entra() {
    let m = Persona::nueva("José").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m, "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0).unwrap();
    let larga = "x".repeat(51);
    assert_eq!(obra.avisar_termino(0, &c, 100, larga), Err(Error::Nota));
}

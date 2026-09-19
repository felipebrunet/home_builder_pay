use super::*;

#[test]
fn renombrar_conserva_id() {
    let mut p = Persona::nueva("José").unwrap();
    let id = p.id.clone();
    p.renombrar("Don Dinero").unwrap();
    assert_eq!(p.id, id);
    assert_eq!(p.nombre, "Don Dinero");
    assert_eq!(p.renombrar("  "), Err(Error::Nombre));
}

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
fn rechazar_contra_vuelve_el_aviso() {
    let m = Persona::nueva("Dinero").unwrap();
    let mid = m.id.clone();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec![]).unwrap();
    let a = Aceptacion::de(&o, c, 1_000).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.estado, EstadoObra::Contra);
    assert_eq!(obra.garantia_publicada, 2_000);
    obra.rechazar_contra(&mid).unwrap();
    assert_eq!(obra.estado, EstadoObra::Rechazada);
    assert!(obra.contra.is_none());
    assert_eq!(obra.n_partidas, 5);
    assert_eq!(obra.partidas.len(), 5);
    assert_eq!(obra.garantia, 2_000);
}

#[test]
fn fusionar_no_revive_partidas_de_contra_rechazada() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 5_000, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
    let contra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(contra.n_partidas, 5);
    let o2 = Oferta::publicar(m, "Casa", 10_000, 5_000, vec![]).unwrap();
    let a2 = Aceptacion::de(&o2, c, 5_000).unwrap();
    let mut acordada = Obra::desde_oferta(o2, a2).unwrap();
    acordada.id = contra.id.clone();
    assert_eq!(acordada.n_partidas, 2);
    acordada.fusionar(contra);
    assert_eq!(acordada.n_partidas, 2);
    assert_eq!(acordada.partidas.len(), 2);
    assert_eq!(acordada.estado, EstadoObra::Acordada);
}

#[test]
fn encerrar_y_pagar_usa_stub_xmr() {
    let m = Persona::nueva("Felipe").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m.clone(), "Muro", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0, &m).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Encerrada);
    obra.avisar_termino(0, &c, 100, "Listo").unwrap();
    obra.aceptar_pago(0, &m).unwrap();
    obra.encerrar_partida(1, &m).unwrap();
    obra.avisar_termino(1, &c, 100, "").unwrap();
    obra.aceptar_pago(1, &m).unwrap();
    assert_eq!(obra.estado, EstadoObra::Cerrada);
    assert_eq!(obra.partidas[0].pago, Some(100));
    let rec = obra.partidas[0].recibo.as_ref().unwrap();
    assert_eq!(rec.porcentaje, 100);
    assert_eq!(rec.acepto_nombre, "Felipe");
    assert!(rec.cuando > 0);
    assert_eq!(obra.partidas[0].encerrado_por.as_ref().unwrap().nombre, "Felipe");
}

#[test]
fn se_puede_abandonar_antes_de_cerrar() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.abandonar(&c).unwrap();
    assert_eq!(obra.estado, EstadoObra::Abandonada);
    assert_eq!(obra.abandonar(&m), Err(Error::YaExiste));
}

#[test]
fn fusionar_no_vuelve_encerrar_atras() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c, 50).unwrap();
    let mut vieja = Obra::desde_oferta(o, a).unwrap();
    let mut nueva = vieja.clone();
    nueva.encerrar_partida(0, &m).unwrap();
    vieja.fusionar(nueva.clone());
    assert_eq!(vieja.partidas[0].estado, PartidaEstado::Encerrada);
    let mut stale = nueva.clone();
    stale.partidas[0].estado = PartidaEstado::Pendiente;
    stale.estado = EstadoObra::Acordada;
    nueva.fusionar(stale);
    assert_eq!(nueva.partidas[0].estado, PartidaEstado::Encerrada);
    assert_eq!(nueva.estado, EstadoObra::EnMarcha);
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
    obra.encerrar_partida(0, &m).unwrap();
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
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0, &m).unwrap();
    let larga = "x".repeat(51);
    assert_eq!(obra.avisar_termino(0, &c, 100, larga), Err(Error::Nota));
}

#[test]
fn se_edita_detalle_pendiente_no_encerrada() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec!["Cimentos".into()]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.editar_detalle(0, &m, "Cimientos").unwrap();
    assert_eq!(obra.partidas[0].detalle, "Cimientos");
    obra.encerrar_partida(0, &m).unwrap();
    assert_eq!(
        obra.editar_detalle(0, &m, "Otra"),
        Err(Error::YaExiste)
    );
}

#[test]
fn partida_extra_suma_trabajo_si_ambos_aceptan() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.n_partidas, 2);
    obra.proponer_extra(&c, "Techumbre extra").unwrap();
    obra.aceptar_extra(&m).unwrap();
    assert_eq!(obra.n_partidas, 3);
    assert_eq!(obra.trabajo, 150);
    assert_eq!(obra.partidas[2].detalle, "Techumbre extra");
    assert_eq!(n_partidas(obra.trabajo, obra.garantia).unwrap(), 3);
    assert_eq!(obra.proponer_extra(&m, "  "), Err(Error::Detalle));
}

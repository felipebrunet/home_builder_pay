use super::*;

fn par(nombre: &str) -> (Persona, String) {
    let mut p = Persona::nueva(nombre).unwrap();
    let (sec, pubk) = generar_clave();
    p.clave_pub = pubk;
    (p, sec)
}

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
    obra.encerrar_proponer(0, &m).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Encerrando);
    obra.encerrar_confirmar(0, &c).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Encerrada);
    obra.avisar_termino(0, &c, 100, "Listo").unwrap();
    obra.aceptar_pago(0, &m).unwrap();
    obra.encerrar_proponer(1, &m).unwrap();
    obra.encerrar_confirmar(1, &c).unwrap();
    obra.avisar_termino(1, &c, 100, "").unwrap();
    obra.aceptar_pago(1, &m).unwrap();
    assert_eq!(obra.estado, EstadoObra::Cerrada);
    assert_eq!(obra.partidas[0].pago, Some(100));
    let rec = obra.partidas[0].recibo.as_ref().unwrap();
    assert_eq!(rec.porcentaje, 100);
    assert_eq!(rec.acepto_nombre, "Felipe");
    assert!(rec.cuando > 0);
    assert_eq!(obra.partidas[0].encerrado_por.as_ref().unwrap().nombre, "Felipe");
    assert!(obra.partidas[0].fondeo_txid.is_none());
}

#[test]
fn fusionar_copia_el_txid_y_no_lo_pisa() {
    let mut local = Partida::pendiente("muro");
    let mut remota = Partida::pendiente("muro");
    remota.fondeo_txid = Some("abc".into());
    local.fusionar(remota);
    assert_eq!(local.fondeo_txid.as_deref(), Some("abc"));
    let mut otra = Partida::pendiente("muro");
    otra.fondeo_txid = Some("zzz".into());
    otra.pago_txid = Some("pago".into());
    local.fusionar(otra);
    assert_eq!(local.fondeo_txid.as_deref(), Some("abc"));
    assert_eq!(local.pago_txid.as_deref(), Some("pago"));
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
fn encerrar_pide_a_los_dos() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_proponer(0, &m).unwrap();
    assert_eq!(obra.encerrar_confirmar(0, &m), Err(Error::NoToca));
    obra.encerrar_confirmar(0, &c).unwrap();
    assert_eq!(obra.partidas[0].estado, PartidaEstado::Encerrada);
}

#[test]
fn abandonar_con_encierre_es_a_dos() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
    obra.abandonar(&c).unwrap();
    assert_eq!(obra.estado, EstadoObra::EnMarcha);
    assert_eq!(obra.cierre.as_ref().unwrap().id, c.id);
    obra.aceptar_cierre(&m).unwrap();
    assert_eq!(obra.estado, EstadoObra::Abandonada);
}

#[test]
fn rechazar_cierre_no_vuelve_con_el_chisme() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
    let mut propuesta = obra.clone();
    propuesta.abandonar(&c).unwrap();
    let mut mandante = propuesta.clone();
    mandante.rechazar_cierre(&m).unwrap();
    mandante.fusionar(propuesta);
    assert!(mandante.cierre.is_none());
    assert_eq!(mandante.estado, EstadoObra::EnMarcha);
}

#[test]
fn fusionar_no_vuelve_encerrar_atras() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut vieja = Obra::desde_oferta(o, a).unwrap();
    let mut nueva = vieja.clone();
    nueva.encerrar_proponer(0, &m).unwrap();
    nueva.encerrar_confirmar(0, &c).unwrap();
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
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
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
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
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
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
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
    obra.proponer_extra(&c, "Techumbre extra", 30).unwrap();
    obra.aceptar_extra(&m).unwrap();
    assert_eq!(obra.n_partidas, 3);
    assert_eq!(obra.trabajo, 130);
    assert_eq!(obra.partidas[2].detalle, "Techumbre extra");
    assert_eq!(obra.partidas[2].capital(obra.garantia), 30);
    assert_eq!(obra.proponer_extra(&m, "  ", 10), Err(Error::Detalle));
}

#[test]
fn rechazar_extra_no_vuelve_con_el_chisme() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut propuesta = Obra::desde_oferta(o, a).unwrap();
    propuesta.proponer_extra(&c, "Extra", 40).unwrap();
    let mut mandante = propuesta.clone();
    mandante.rechazar_extra(&m).unwrap();
    assert!(mandante.extra.is_none());
    mandante.fusionar(propuesta);
    assert!(mandante.extra.is_none());
}

#[test]
fn obra_en_curso_sale_del_tablero() {
    let (m, _) = par("Alice");
    let (c, _) = par("Bob");
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec![]).unwrap();
    let id = o.id.clone();
    assert!(oferta_en_tablero(&id, &[]));
    let a = Aceptacion::de(&o, c, 1_000).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.estado, EstadoObra::Contra);
    assert!(!oferta_en_tablero(&id, std::slice::from_ref(&obra)));
    assert!(obra.participa(&m.id));
    assert!(!obra.participa("carol"));
    obra.rechazar_contra(&m.id).unwrap();
    assert!(oferta_en_tablero(&id, std::slice::from_ref(&obra)));
}

#[test]
fn carol_ve_la_caja_y_no_el_texto() {
    let (m, ms) = par("Alice");
    let (c, cs) = par("Bob");
    let (_carol, carol_s) = par("Carol");
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec!["Muro".into()]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_proponer(0, &m).unwrap();
    obra.encerrar_confirmar(0, &c).unwrap();
    obra.avisar_termino(0, &c, 100, "Terminé el muro").unwrap();
    obra.preparar_para_red(&c.id, &c.clave_pub, &cs).unwrap();
    assert!(!obra.partidas[0].notas[0].caja.contains("Terminé"));
    assert!(obra.partidas[0].notas[0].texto.is_empty());
    assert!(!obra.partidas[0].notas[0].caja.is_empty());
    match obra.leer_nota(&obra.partidas[0].notas[0], &cs) {
        TextoLeido::Plano(t) => assert_eq!(t, "Terminé el muro"),
        TextoLeido::Cerrado => panic!("bob no pudo abrir su nota"),
    }
    match obra.leer_nota(&obra.partidas[0].notas[0], &ms) {
        TextoLeido::Plano(t) => assert_eq!(t, "Terminé el muro"),
        TextoLeido::Cerrado => panic!("alice no pudo abrir la nota"),
    }
    match obra.leer_nota(&obra.partidas[0].notas[0], &carol_s) {
        TextoLeido::Cerrado => {}
        TextoLeido::Plano(t) => panic!("carol leyó {t}"),
    }

    let mut clara = obra.clone();
    clara.partidas[0].notas[0].caja.clear();
    clara.partidas[0].notas[0].texto = "Terminé el muro".into();
    clara.fusionar(obra.clone());
    assert!(clara.partidas[0].notas[0].texto.is_empty());
    assert!(!clara.partidas[0].notas[0].caja.is_empty());
}

#[test]
fn el_chisme_vacio_no_borra_la_nota_local() {
    let (m, _) = par("Alice");
    let (c, _) = par("Bob");
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec!["Muro".into()]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
    let mut local = Obra::desde_oferta(o, a).unwrap();
    local.encerrar_proponer(0, &m).unwrap();
    local.encerrar_confirmar(0, &c).unwrap();
    local.avisar_termino(0, &c, 80, "Terminé el muro").unwrap();
    local.proponer_extra(&c, "Techumbre secreta", 30).unwrap();

    let mut eco = local.sin_texto_claro();
    assert!(eco.partidas[0].notas[0].texto.is_empty());
    assert!(eco.extra.as_ref().unwrap().detalle.is_empty());
    assert_eq!(eco.partidas[0].detalle, "Muro");
    eco.partidas[0].estado = PartidaEstado::Pagada;
    local.fusionar(eco);
    assert_eq!(local.partidas[0].notas[0].texto, "Terminé el muro");
    assert_eq!(
        local.extra.as_ref().unwrap().detalle,
        "Techumbre secreta"
    );
    assert_eq!(local.partidas[0].estado, PartidaEstado::Pagada);
}

#[test]
fn extra_cifrada_vuelve_al_aceptar() {
    let (m, ms) = par("Alice");
    let (c, cs) = par("Bob");
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.proponer_extra(&c, "Techumbre extra", 30).unwrap();
    obra.preparar_para_red(&c.id, &c.clave_pub, &cs).unwrap();
    assert!(obra.extra.as_ref().unwrap().detalle.is_empty());
    assert!(!obra
        .extra
        .as_ref()
        .unwrap()
        .detalle_caja
        .contains("Techumbre"));
    match obra.leer_extra(&ms) {
        TextoLeido::Plano(t) => assert_eq!(t, "Techumbre extra"),
        TextoLeido::Cerrado => panic!("alice no pudo abrir el extra"),
    }
    obra.abrir_extra(&ms).unwrap();
    obra.aceptar_extra(&m).unwrap();
    assert_eq!(obra.partidas.last().unwrap().detalle, "Techumbre extra");
    assert_eq!(obra.trabajo, 130);
}

#[test]
fn extra_no_entra_si_esta_abandonada() {
    let m = Persona::nueva("Dinero").unwrap();
    let c = Persona::nueva("Chasquilla").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 100, 50, vec![]).unwrap();
    let a = Aceptacion::de(&o, c.clone(), 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.proponer_extra(&c, "Extra", 40).unwrap();
    obra.abandonar(&m).unwrap();
    assert!(obra.extra.is_none());
    assert_eq!(obra.aceptar_extra(&c), Err(Error::NoToca));
}

// ── Obras en USD: precio fijado al proponer el encierre ──

fn obra_usd() -> (Obra, Persona, Persona) {
    let m = Persona::nueva("Felipe").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    // USD 100 de trabajo, USD 50 de garantía por partida: 2 partidas.
    let o = Oferta::publicar_usd(m.clone(), "Casa", 10_000, 5_000, vec![]).unwrap();
    assert_eq!(o.moneda, Moneda::Usd);
    let a = Aceptacion::de(&o, c.clone(), 5_000).unwrap();
    let obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.moneda, Moneda::Usd);
    (obra, m, c)
}

#[test]
fn usd_pide_precio_para_encerrar_y_fija_el_xmr() {
    let (mut obra, m, c) = obra_usd();
    assert_eq!(obra.piconero_partida(0), None);
    assert_eq!(obra.encerrar_proponer(0, &m), Err(Error::Precio));
    // Precio con dólares que no son los de la partida: no.
    let malo = PrecioFijado::nuevo(4_000, 16_000, "coingecko", 1).unwrap();
    assert_eq!(obra.encerrar_proponer_con(0, &m, Some(malo)), Err(Error::Precio));
    // Precio inconsistente (piconero tocado): no.
    let mut tocado = PrecioFijado::nuevo(5_000, 16_000, "coingecko", 1).unwrap();
    tocado.piconero += 7;
    assert_eq!(obra.encerrar_proponer_con(0, &m, Some(tocado)), Err(Error::Precio));
    let pr = PrecioFijado::nuevo(5_000, 16_000, "coingecko", 1).unwrap();
    obra.encerrar_proponer_con(0, &m, Some(pr.clone())).unwrap();
    // USD 50 a USD 160/XMR = 0,3125 XMR por lado.
    assert_eq!(obra.piconero_partida(0), Some(312_500_000_000));
    obra.encerrar_confirmar(0, &c).unwrap();
    assert_eq!(obra.partidas[0].precio.as_ref(), Some(&pr));
}

#[test]
fn usd_los_dos_terminan_con_el_mismo_xmr() {
    let (mut a, m, c) = obra_usd();
    let mut b = a.clone();
    let pr = PrecioFijado::nuevo(5_000, 15_437, "kraken", 7).unwrap();
    a.encerrar_proponer_con(0, &m, Some(pr)).unwrap();
    b.fusionar(a.clone());
    assert_eq!(b.piconero_partida(0), a.piconero_partida(0));
    b.encerrar_confirmar(0, &c).unwrap();
    a.fusionar(b.clone());
    assert_eq!(a.partidas[0].estado, PartidaEstado::Encerrada);
    assert_eq!(a.piconero_partida(0), b.piconero_partida(0));
}

#[test]
fn usd_copia_sin_precio_no_borra_el_fijado() {
    // Un par viejo reenvía la obra sin el campo `precio` (y sin `moneda`).
    let (mut a, m, c) = obra_usd();
    let pr = PrecioFijado::nuevo(5_000, 16_000, "coingecko", 1).unwrap();
    a.encerrar_proponer_con(0, &m, Some(pr.clone())).unwrap();
    let mut vieja = a.clone();
    vieja.encerrar_confirmar(0, &c).unwrap();
    vieja.moneda = Moneda::Unidades;
    for p in &mut vieja.partidas {
        p.precio = None;
    }
    a.fusionar(vieja);
    assert_eq!(a.moneda, Moneda::Usd);
    assert_eq!(a.partidas[0].estado, PartidaEstado::Encerrada);
    assert_eq!(a.partidas[0].precio, Some(pr));
}

#[test]
fn usd_cancelar_borra_el_precio_en_los_dos() {
    let (mut a, m, c) = obra_usd();
    let pr = PrecioFijado::nuevo(5_000, 16_000, "coingecko", 1).unwrap();
    a.encerrar_proponer_con(0, &m, Some(pr)).unwrap();
    let mut b = a.clone();
    b.encerrar_cancelar(0, &c).unwrap();
    assert!(b.partidas[0].precio.is_none());
    a.fusionar(b);
    assert_eq!(a.partidas[0].estado, PartidaEstado::Pendiente);
    assert!(a.partidas[0].precio.is_none());
    // Se puede volver a proponer con otro precio.
    let otro = PrecioFijado::nuevo(5_000, 20_000, "kraken", 2).unwrap();
    a.encerrar_proponer_con(0, &c, Some(otro)).unwrap();
    assert_eq!(a.piconero_partida(0), Some(250_000_000_000));
}

#[test]
fn usd_propuestas_cruzadas_con_precios_distintos_convergen() {
    // Los dos proponen a la vez con su propio precio: después de fusionar en
    // las dos direcciones quedan con el mismo proponente y el mismo XMR.
    let (base, m, c) = obra_usd();
    let mut a = base.clone();
    let mut b = base;
    a.encerrar_proponer_con(0, &m, Some(PrecioFijado::nuevo(5_000, 16_000, "coingecko", 1).unwrap()))
        .unwrap();
    b.encerrar_proponer_con(0, &c, Some(PrecioFijado::nuevo(5_000, 17_000, "kraken", 2).unwrap()))
        .unwrap();
    let (a0, b0) = (a.clone(), b.clone());
    a.fusionar(b0);
    b.fusionar(a0);
    assert_eq!(a.partidas[0].encerrado_por, b.partidas[0].encerrado_por);
    assert_eq!(a.partidas[0].precio, b.partidas[0].precio);
    assert_eq!(a.piconero_partida(0), b.piconero_partida(0));
}

#[test]
fn obra_vieja_sigue_en_unidades() {
    let m = Persona::nueva("Felipe").unwrap();
    let c = Persona::nueva("Juan").unwrap();
    let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec![]).unwrap();
    let a = Aceptacion::de(&o, c, 2_000).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    assert_eq!(obra.moneda, Moneda::Unidades);
    // Sin campo moneda en el JSON (0.2.9): unidades.
    let json = serde_json::to_string(&obra).unwrap();
    assert!(!json.contains("moneda"));
    let leida: Obra = serde_json::from_str(&json).unwrap();
    assert_eq!(leida.moneda, Moneda::Unidades);
    obra.encerrar_proponer(0, &m).unwrap();
    assert!(obra.partidas[0].precio.is_none());
    // 2000 unidades = 0,04 XMR.
    assert_eq!(obra.piconero_partida(0), Some(40_000_000_000));
}

#[test]
fn oferta_usd_viaja_con_su_moneda() {
    let m = Persona::nueva("Felipe").unwrap();
    let o = Oferta::publicar_usd(m, "Casa", 10_000, 5_000, vec![]).unwrap();
    let json = serde_json::to_string(&o).unwrap();
    assert!(json.contains("\"moneda\":\"usd\""));
    let leida: Oferta = serde_json::from_str(&json).unwrap();
    assert_eq!(leida.moneda, Moneda::Usd);
}

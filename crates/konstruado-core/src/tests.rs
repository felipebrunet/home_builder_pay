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
    let o = Oferta::publicar(m, "Casa", 10_000, 2_000).unwrap();
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
    let o = Oferta::publicar(m, "Casa", 10_000, 2_000).unwrap();
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
    let o = Oferta::publicar(m, "Muro", 100, 50).unwrap();
    let a = Aceptacion::de(&o, c, 50).unwrap();
    let mut obra = Obra::desde_oferta(o, a).unwrap();
    obra.encerrar_partida(0).unwrap();
    assert_eq!(obra.partidas[0], PartidaEstado::Encerrada);
    obra.pagar_partida(0).unwrap();
    obra.encerrar_partida(1).unwrap();
    obra.pagar_partida(1).unwrap();
    assert_eq!(obra.estado, EstadoObra::Cerrada);
}

package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAbrirArchivo
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.rememberGuardarArchivo
import cl.konstruado.app.ui.sondear

@Composable
private fun Restaurar(acciones: Acciones, banner: Banner) {
    val app = AppHolder.a
    val abrirSemilla = rememberAbrirArchivo(acciones) { app.restaurarSemilla(it) }
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    val guardarObras = rememberGuardarArchivo(
        acciones, banner, { app.exportarObras() },
        "Guardé el respaldo de obras. No incluye seed ni share; puede estar desfasado vs el otro.",
    )
    val abrirObras = rememberAbrirArchivo(acciones) { app.importarObras(it) }
    Pista("Recuperar las 25 palabras trae tu dirección personal. No trae la caja de la obra ni tu nombre en el trato.")
    Secundario("Recuperar las 25 palabras") { abrirSemilla() }
    Pista("Recuperar un share trae la caja de una obra que ya está en este equipo. Tiene que ser el tuyo: el del otro lado no sirve.")
    Secundario("Recuperar un share") { abrirShare() }
    Pista("Respaldo de obras: el perfil (obras/ofertas) para reinstalar. Puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero. No incluye seed ni share.")
    Secundario("Guardar respaldo de obras") { guardarObras("konstruado-obras.json") }
    Secundario("Recuperar respaldo de obras") { abrirObras() }
}

@Composable
fun BilleteraScreen(banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear { app.billetera() } ?: run { Pista("Cargando…"); return }
    val b = r.getOrElse { ErrorTexto(it.humano()); return }
    var destino by remember { mutableStateOf("") }
    var monto by remember { mutableStateOf("") }
    val guardarPalabras = rememberGuardarArchivo(
        acciones, banner, { app.exportarSemilla() }, "Guardé las 25 palabras en el archivo que elegiste.",
    )
    Titulo("Billetera")
    Lead("Tu Monero personal de stagenet. La caja de una obra es otra dirección, de las dos personas.")
    val esDefecto = AppHolder.a.daemonEsDefecto()
    Pista("Nodo: ${b.daemon}" + if (esDefecto) " (público)" else " (propio · cambiá en Cuenta)")
    val pruebaNodo = sondear { app.ultimaPruebaDaemon() }?.getOrNull()
    if (pruebaNodo != null) {
        if (pruebaNodo.ok) {
            Pista("Última prueba RPC: OK · bloque ${pruebaNodo.tip} · ${pruebaNodo.ms} ms")
        } else {
            ErrorTexto("Última prueba RPC falló (${pruebaNodo.ms} ms): ${pruebaNodo.mensaje}")
        }
    } else if (!esDefecto) {
        Pista("Si el saldo no carga, en Cuenta tocá «Probar RPC del nodo» contra esta URL.")
    }
    val dir = b.direccion
    if (dir == null) {
        Pista(b.escala)
        Pista("Todavía no hay semilla en este equipo. Se crean 25 palabras nuevas y quedan en el almacenamiento privado de la app.")
        Primario("Crear billetera de stagenet") { acciones.correr("Billetera creada. Guardá las 25 palabras.") { app.crearSemilla() } }
        Restaurar(acciones, banner)
        return
    }
    Text("${b.total} XMR", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.SemiBold)
    Text("Libre ${b.libre} · trabado ${b.trabado} (10 bloques)")
    b.tip?.let { Pista("Punta del nodo: $it") }
    Pista(b.visto)
    if (b.buscando) Pista("Mirando la cadena…")
    if (b.enviando) Pista("Firmando y publicando…")
    b.retro?.let { Pista(it) }
    b.aviso?.let { ErrorTexto(it) }
    b.ultimo?.let { Pista(it) }
    if (b.movs.isNotEmpty()) {
        Seccion("Entradas")
        b.movs.forEach { m -> Text("${m.monto} · ${m.detalle}", style = MaterialTheme.typography.bodySmall) }
    }
    Seccion("1 · Recibir")
    Copiable("Tu dirección stagenet", dir)
    Pista("El scan no ve monedas más viejas que lo que ya miramos: si el faucet es viejo, pedí mirar más atrás.")
    Pista("Al guardar las 25 palabras también queda la altura de bloque del nodo; al recuperar, el scan parte de ahí (no desde el génesis).")
    Secundario("Guardar las 25 palabras") { guardarPalabras("konstruado-semilla.txt") }
    Restaurar(acciones, banner)
    Seccion("2 · Enviar")
    OutlinedTextField(destino, { destino = it }, label = { Text("Dirección de stagenet") }, modifier = Modifier.fillMaxWidth())
    OutlinedTextField(monto, { monto = it }, label = { Text("Monto en XMR") }, placeholder = { Text("0.04") }, modifier = Modifier.fillMaxWidth())
    Pista("El cambio vuelve a esta billetera. Se reserva 0,001 XMR para el fee.")
    Secundario("Usar el máximo") {
        acciones.pedir({ app.maximoEnvio() }) { m ->
            if (m == null) banner.error.value = "No hay saldo libre suficiente para el fee." else monto = m
        }
    }
    Primario("Enviar") { acciones.correr("Envío pedido. Mirá el estado arriba.") { app.enviar(destino, monto) } }
    Secundario("Actualizar saldo") { acciones.correr { app.actualizarSaldo() } }
    Secundario("Mirar 200 bloques más atrás") { acciones.correr("Sumé 200 bloques hacia atrás.") { app.mirarAtras() } }
    if (b.cajas.isNotEmpty()) {
        Seccion("Cajas de obras (2-de-2)")
        b.cajas.forEach { c ->
            Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Column(Modifier.padding(12.dp)) {
                    Text(c.obraNombre, fontWeight = FontWeight.SemiBold)
                    CajaPanel(c.obraId, c.direccion, c.mirada, acciones, banner)
                }
            }
        }
    }
}

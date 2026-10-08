package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ReinicioActivity
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.DatoFila
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.Peligro
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.rememberAbrirBytes
import cl.konstruado.app.ui.rememberGuardarBytes
import cl.konstruado.app.ui.sondear
import cl.konstruado.app.ui.tonoDe
import uniffi.konstruado_ffi.RespaldoResumen

@Composable
private fun CampoClave(valor: String, etiqueta: String, cambiar: (String) -> Unit) {
    OutlinedTextField(
        valor, cambiar, label = { Text(etiqueta) }, singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** Exportar el respaldo completo (un archivo cifrado). Regla y textos: `respaldo.rs`. */
@Composable
fun RespaldoCompleto(acciones: Acciones, banner: Banner) {
    val app = AppHolder.a
    val est = sondear { app.estadoRespaldo() }?.getOrNull() ?: return
    var clave by remember { mutableStateOf("") }
    var clave2 by remember { mutableStateOf("") }
    val guardar = rememberGuardarBytes(
        acciones, banner,
        contenido = { app.exportarRespaldo(clave) },
        alGuardar = { app.respaldoGuardado(); clave = ""; clave2 = "" },
        okMsg = "Respaldo completo guardado en el archivo que elegiste.",
    )
    Text("Respaldo completo", style = androidx.compose.material3.MaterialTheme.typography.titleSmall)
    EstadoFila(tonoDe(est.tono), est.linea)
    est.ayuda.take(2).forEach { Ayuda(it) }
    CampoClave(clave, "Contraseña (al menos ${est.claveMinima})") { clave = it }
    CampoClave(clave2, "Repetila") { clave2 = it }
    Primario("Exportar respaldo completo") {
        when {
            clave != clave2 -> banner.error.value = "Las dos contraseñas no coinciden."
            clave.length < est.claveMinima.toInt() -> banner.error.value = "La contraseña tiene que tener al menos ${est.claveMinima} caracteres."
            else -> guardar(est.nombreArchivo)
        }
    }
    ComoFunciona("Cuándo exportarlo de nuevo") {
        est.ayuda.drop(2).forEach { Ayuda(it) }
    }
}

/**
 * Restaurar desde el respaldo completo: elegir archivo, contraseña, revisar,
 * confirmar (peligro si ya hay datos) y reiniciar. Bienvenida y Billetera.
 */
@Composable
fun RestaurarRespaldo(acciones: Acciones, banner: Banner) {
    val app = AppHolder.a
    val ctx = LocalContext.current
    var archivo by remember { mutableStateOf<Pair<String, ByteArray>?>(null) }
    var clave by remember { mutableStateOf("") }
    var resumen by remember { mutableStateOf<RespaldoResumen?>(null) }
    var confirma by remember { mutableStateOf(false) }
    var listo by remember { mutableStateOf(false) }
    val abrir = rememberAbrirBytes(acciones) { n, b ->
        archivo = n to b
        resumen = null
        confirma = false
    }
    val restaurar = {
        val (_, b) = archivo!!
        val reemplazar = resumen?.hayDatos == true
        acciones.correr("Respaldo restaurado. Konstruado se reinicia…", alTerminar = {
            listo = true
            clave = ""
            ReinicioActivity.reiniciar(ctx)
        }) { app.restaurarRespaldo(b, clave, reemplazar) }
    }
    Text("Restaurar desde respaldo", style = androidx.compose.material3.MaterialTheme.typography.titleSmall)
    Ayuda("Elegí el archivo .kbak y escribí su contraseña. Primero se revisa todo (semilla, cada share contra su obra y tu rol); si algo no cuadra no se escribe nada.")
    Secundario(if (archivo == null) "Elegir el archivo del respaldo" else "Elegir otro archivo") { abrir() }
    val a = archivo
    if (a != null) {
        Ayuda(a.first, maxLines = 1)
        CampoClave(clave, "Contraseña del respaldo") { clave = it; resumen = null; confirma = false }
        if (resumen == null) {
            Primario("Abrir y revisar", enabled = clave.isNotEmpty()) {
                acciones.pedir({ app.revisarRespaldo(a.second, clave) }) { resumen = it }
            }
        }
    }
    val r = resumen
    if (r != null && !listo) {
        Divisor()
        DatoFila("Cuenta", "${r.nombre} · ${r.rol}")
        DatoFila("Obras · ofertas", "${r.nObras} · ${r.nOfertas}")
        DatoFila("Cajas (shares)", "${r.nShares}")
        r.direccion?.let { DatoFila("Billetera", it.take(12) + "…", mono = true) }
        DatoFila("Mirar desde el bloque", r.altura?.toString() ?: "—", mono = true)
        DatoFila("Hecho", "${r.creado} · v${r.app}")
        if (r.hayDatos) {
            EstadoFila(Tono.Error, "Este equipo ya tiene una cuenta, billetera o cajas. Restaurar las reemplaza enteras (no se mezclan).")
            Ayuda("Lo de ahora queda guardado en la carpeta previo-… de los datos de la app.")
            if (confirma) {
                Peligro("Sí, reemplazar todo y reiniciar", lleno = true) { restaurar() }
                TextoBoton("No") { confirma = false }
            } else {
                Peligro("Reemplazar lo de este equipo") { confirma = true }
            }
        } else {
            Primario("Restaurar y reiniciar") { restaurar() }
        }
        Ayuda("Después de restaurar, lo más nuevo del trato baja del otro por la sala cuando los dos están en línea.")
    }
}

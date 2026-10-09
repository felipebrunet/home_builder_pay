package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
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
        okMsg = tr("Respaldo completo guardado en el archivo que elegiste.", "Full backup saved to the file you chose."),
    )
    Text(tr("Respaldo completo", "Full backup"), style = androidx.compose.material3.MaterialTheme.typography.titleSmall)
    EstadoFila(tonoDe(est.tono), est.linea)
    est.ayuda.take(2).forEach { Ayuda(it) }
    CampoClave(clave, tr("Contraseña (al menos ${est.claveMinima})", "Password (at least ${est.claveMinima})")) { clave = it }
    CampoClave(clave2, tr("Repetila", "Repeat it")) { clave2 = it }
    Primario(tr("Exportar respaldo completo", "Export full backup")) {
        when {
            clave != clave2 -> banner.error.value = tr("Las dos contraseñas no coinciden.", "The two passwords do not match.")
            clave.length < est.claveMinima.toInt() -> banner.error.value = tr("La contraseña tiene que tener al menos ${est.claveMinima} caracteres.", "The password needs at least ${est.claveMinima} characters.")
            else -> guardar(est.nombreArchivo)
        }
    }
    ComoFunciona(tr("Cuándo exportarlo de nuevo", "When to export it again")) {
        est.ayuda.drop(2).forEach { Ayuda(it) }
    }
}

/**
 * Restaurar desde el respaldo completo: elegir archivo, contraseña, revisar,
 * confirmar (peligro si ya hay datos) y reiniciar. Bienvenida y Billetera.
 */
@Composable
fun RestaurarRespaldo(acciones: Acciones) {
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
        acciones.correr(tr("Respaldo restaurado. Konstruado se reinicia…", "Backup restored. Konstruado restarts…"), alTerminar = {
            listo = true
            clave = ""
            ReinicioActivity.reiniciar(ctx)
        }) { app.restaurarRespaldo(b, clave, reemplazar) }
    }
    Text(tr("Restaurar desde respaldo", "Restore from backup"), style = androidx.compose.material3.MaterialTheme.typography.titleSmall)
    Ayuda(tr("Elegí el archivo .kbak y escribí su contraseña. Primero se revisa todo (semilla, cada share contra su obra y tu rol); si algo no cuadra no se escribe nada.", "Choose the .kbak file and type its password. Everything is checked first (seed, each share against its job and your role); if something does not match nothing is written."))
    Secundario(if (archivo == null) tr("Elegir el archivo del respaldo", "Choose the backup file") else tr("Elegir otro archivo", "Choose another file")) { abrir() }
    val a = archivo
    if (a != null) {
        Ayuda(a.first, maxLines = 1)
        CampoClave(clave, tr("Contraseña del respaldo", "Backup password")) { clave = it; resumen = null; confirma = false }
        if (resumen == null) {
            Primario(tr("Abrir y revisar", "Open and check"), enabled = clave.isNotEmpty()) {
                acciones.pedir({ app.revisarRespaldo(a.second, clave) }) { resumen = it }
            }
        }
    }
    val r = resumen
    if (r != null && !listo) {
        Divisor()
        DatoFila(tr("Cuenta", "Account"), "${r.nombre} · ${r.rol}")
        DatoFila(tr("Obras · ofertas", "Jobs · offers"), "${r.nObras} · ${r.nOfertas}")
        DatoFila(tr("Cajas (shares)", "Boxes (shares)"), "${r.nShares}")
        r.direccion?.let { DatoFila(tr("Billetera", "Wallet"), it.take(12) + "…", mono = true) }
        DatoFila(tr("Mirar desde el bloque", "Scan from block"), r.altura?.toString() ?: "—", mono = true)
        DatoFila(tr("Hecho", "Created"), "${r.creado} · v${r.app}")
        if (r.hayDatos) {
            EstadoFila(Tono.Error, tr("Este equipo ya tiene una cuenta, billetera o cajas. Restaurar las reemplaza enteras (no se mezclan).", "This device already has an account, wallet or boxes. Restoring replaces them entirely (nothing is merged)."))
            Ayuda(tr("Lo de ahora queda guardado en la carpeta previo-… de los datos de la app.", "The current data is kept in the previo-… folder of the app data."))
            if (confirma) {
                Peligro(tr("Sí, reemplazar todo y reiniciar", "Yes, replace everything and restart"), lleno = true) { restaurar() }
                TextoBoton(tr("No", "No")) { confirma = false }
            } else {
                Peligro(tr("Reemplazar lo de este equipo", "Replace this device's data")) { confirma = true }
            }
        } else {
            Primario(tr("Restaurar y reiniciar", "Restore and restart")) { restaurar() }
        }
        Ayuda(tr("Después de restaurar, lo más nuevo del trato baja del otro por la sala cuando los dos están en línea.", "After restoring, the latest deal state arrives from the other side through the room when both are online."))
    }
}

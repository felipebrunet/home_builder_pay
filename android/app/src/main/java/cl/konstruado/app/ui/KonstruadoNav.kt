package cl.konstruado.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AccountBalanceWallet
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.screens.BienvenidaScreen
import cl.konstruado.app.ui.screens.BilleteraScreen
import cl.konstruado.app.ui.screens.CuentaScreen
import cl.konstruado.app.ui.screens.ObraScreen
import cl.konstruado.app.ui.screens.OfertaScreen
import cl.konstruado.app.ui.screens.PartidaScreen
import cl.konstruado.app.ui.screens.PublicarScreen
import cl.konstruado.app.ui.screens.TableroScreen

sealed class Pantalla(val titulo: String) {
    data object Tablero : Pantalla("Tablero")
    data object Billetera : Pantalla("Billetera")
    data object Cuenta : Pantalla("Cuenta")
    data object Publicar : Pantalla("Publicar obra")
    data class Oferta(val id: String) : Pantalla("Oferta")
    data class Obra(val id: String) : Pantalla("Obra")
    data class Partida(val obra: String, val indice: UInt) : Pantalla("Partida")
}

/** Navegación simple con pila propia. */
class Nav {
    val pila = mutableStateListOf<Pantalla>(Pantalla.Tablero)
    val actual: Pantalla get() = pila.last()
    fun ir(p: Pantalla) { pila.add(p) }
    fun raiz(p: Pantalla) { pila.clear(); pila.add(p) }
    fun atras(): Boolean = if (pila.size > 1) { pila.removeAt(pila.lastIndex); true } else false
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun KonstruadoNav(errorArranque: String?) {
    if (errorArranque != null) {
        Column(Modifier.fillMaxSize().padding(24.dp)) {
            Titulo("Konstruado")
            ErrorTexto(errorArranque)
        }
        return
    }
    val app = AppHolder.a
    val banner = remember { Banner() }
    val adentro = remember { mutableStateOf(app.perfil().tieneCuenta) }
    if (!adentro.value) {
        BienvenidaScreen(banner) { adentro.value = true }
        return
    }
    val nav = remember { Nav() }
    BackHandler(enabled = nav.pila.size > 1) { nav.atras() }
    val raiz = nav.pila.size == 1
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (raiz) "Konstruado · ${nav.actual.titulo}" else nav.actual.titulo) },
                navigationIcon = {
                    if (!raiz) IconButton(onClick = { nav.atras() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver")
                    }
                },
            )
        },
        bottomBar = {
            NavigationBar {
                listOf(
                    Triple(Pantalla.Tablero, Icons.Filled.Dashboard, "Tablero"),
                    Triple(Pantalla.Billetera, Icons.Filled.AccountBalanceWallet, "Billetera"),
                    Triple(Pantalla.Cuenta, Icons.Filled.Person, "Cuenta"),
                ).forEach { (p, icono, et) ->
                    NavigationBarItem(
                        selected = nav.pila.first() == p,
                        onClick = { banner.error.value = null; banner.ok.value = null; nav.raiz(p) },
                        icon = { Icon(icono, contentDescription = et) },
                        label = { Text(et) },
                    )
                }
            }
        },
    ) { pad: PaddingValues ->
        Column(
            Modifier.padding(pad).fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)
        ) {
            BannerVista(banner)
            when (val p = nav.actual) {
                Pantalla.Tablero -> TableroScreen(nav, banner)
                Pantalla.Billetera -> BilleteraScreen(banner)
                Pantalla.Cuenta -> CuentaScreen(banner)
                Pantalla.Publicar -> PublicarScreen(nav, banner)
                is Pantalla.Oferta -> OfertaScreen(p.id, nav, banner)
                is Pantalla.Obra -> ObraScreen(p.id, nav, banner)
                is Pantalla.Partida -> PartidaScreen(p.obra, p.indice, banner)
            }
        }
    }
}

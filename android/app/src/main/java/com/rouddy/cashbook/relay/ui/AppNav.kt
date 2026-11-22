package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.dropUnlessResumed
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.SourceType
import kotlinx.serialization.Serializable
import kotlin.reflect.KClass

@Serializable
object DashboardRoute

@Serializable
object EntryRoute

@Serializable
object RecentRoute

@Serializable
object SettingsRoute

@Serializable
data class RuleRoute(val id: String? = null, val source: String = SourceType.SMS.name)

/** 하단 메뉴로 오가는 화면 */
private enum class TopLevel(val route: Any, val routeClass: KClass<*>, val label: String, val icon: ImageVector) {
    DASHBOARD(DashboardRoute, DashboardRoute::class, "대시보드", Icons.Default.Home),
    ENTRY(EntryRoute, EntryRoute::class, "수동 입력", Icons.Default.Edit),
    RECENT(RecentRoute, RecentRoute::class, "최근 내역", Icons.AutoMirrored.Filled.List),
    SETTINGS(SettingsRoute, SettingsRoute::class, "설정", Icons.Default.Settings),
}

/** 하단 메뉴 이동: 화면마다 상태(스크롤·입력 중인 값)를 남겨 두고 돌아오면 되살린다 */
private fun NavHostController.goTo(top: TopLevel) {
    navigate(top.route) {
        popUpTo(graph.findStartDestination().id) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}

@Composable
fun AppNav(openHistoryRequest: Int) {
    val nav = rememberNavController()
    var settingsTab by rememberSaveable { mutableStateOf(SettingsTab.SERVER) }
    val entry by nav.currentBackStackEntryAsState()
    val current = TopLevel.entries.firstOrNull { top -> entry?.destination?.hierarchy?.any { it.hasRoute(top.routeClass) } == true }
    val config by Graph.store.configFlow.collectAsStateWithLifecycle(initialValue = null)
    val alerts = rememberSettingsAlerts(config)

    fun openSettings(tab: SettingsTab) {
        settingsTab = tab
        nav.goTo(TopLevel.SETTINGS)
    }

    LaunchedEffect(openHistoryRequest) {
        if (openHistoryRequest > 0) openSettings(SettingsTab.HISTORY)
    }

    Scaffold(
        // 위쪽 여백은 화면마다 TopAppBar가, 아래쪽은 하단 메뉴(없으면 각 화면)가 맡는다
        contentWindowInsets = WindowInsets(0),
        bottomBar = {
            if (current != null) {
                NavigationBar {
                    TopLevel.entries.forEach { top ->
                        NavigationBarItem(
                            selected = current == top,
                            onClick = { nav.goTo(top) },
                            icon = {
                                BadgedBox(
                                    badge = {
                                        if (top == TopLevel.SETTINGS && alerts.failed > 0) {
                                            Badge { Text("${alerts.failed}") }
                                        } else if (top == TopLevel.SETTINGS && alerts.any) {
                                            Badge()
                                        }
                                    },
                                ) { Icon(top.icon, contentDescription = null) }
                            },
                            label = { Text(top.label) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        NavHost(
            nav,
            startDestination = DashboardRoute,
            modifier = Modifier.padding(padding).consumeWindowInsets(padding),
        ) {
            composable<DashboardRoute> { DashboardScreen(onOpenSettings = { openSettings(SettingsTab.SERVER) }) }
            composable<EntryRoute> { EntryScreen(onOpenSettings = { openSettings(SettingsTab.SERVER) }) }
            composable<RecentRoute> { RecentScreen(onOpenSettings = { openSettings(SettingsTab.SERVER) }) }
            composable<SettingsRoute> {
                SettingsScreen(
                    tab = settingsTab,
                    onTabChange = { settingsTab = it },
                    onEditRule = { rule -> nav.navigate(RuleRoute(rule.id, rule.source.name)) },
                    onAddRule = { source -> nav.navigate(RuleRoute(null, source.name)) },
                )
            }
            composable<RuleRoute> { backStackEntry ->
                val route = backStackEntry.toRoute<RuleRoute>()
                RuleEditScreen(
                    ruleId = route.id,
                    source = SourceType.valueOf(route.source),
                    onDone = dropUnlessResumed { nav.popBackStack() },
                )
            }
        }
    }
}

package com.rouddy.cashbook.relay.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.compose.dropUnlessResumed
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.rouddy.cashbook.relay.data.SourceType
import kotlinx.serialization.Serializable

@Serializable
object HomeRoute

@Serializable
data class RuleRoute(val id: String? = null, val source: String = SourceType.SMS.name)

@Serializable
object HistoryRoute

@Serializable
object SettingsRoute

@Composable
fun AppNav(openHistoryRequest: Int) {
    val nav = rememberNavController()
    LaunchedEffect(openHistoryRequest) {
        if (openHistoryRequest > 0) nav.navigate(HistoryRoute) { launchSingleTop = true }
    }
    NavHost(nav, startDestination = HomeRoute) {
        composable<HomeRoute> {
            HomeScreen(
                onEdit = { rule -> nav.navigate(RuleRoute(rule.id, rule.source.name)) },
                onAdd = { source -> nav.navigate(RuleRoute(null, source.name)) },
                onHistory = { nav.navigate(HistoryRoute) { launchSingleTop = true } },
                onSettings = { nav.navigate(SettingsRoute) { launchSingleTop = true } },
            )
        }
        composable<RuleRoute> { entry ->
            val route = entry.toRoute<RuleRoute>()
            RuleEditScreen(
                ruleId = route.id,
                source = SourceType.valueOf(route.source),
                onDone = dropUnlessResumed { nav.popBackStack() },
            )
        }
        composable<HistoryRoute> {
            HistoryScreen(onBack = dropUnlessResumed { nav.popBackStack() })
        }
        composable<SettingsRoute> {
            SettingsScreen(onBack = dropUnlessResumed { nav.popBackStack() })
        }
    }
}

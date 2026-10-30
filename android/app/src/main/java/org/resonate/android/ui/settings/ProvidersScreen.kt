package org.resonate.android.ui.settings

import androidx.compose.ui.Modifier
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.SignalCellularAlt
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.testTag
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ProviderAccountBook
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

class ProvidersViewModel(resonate: Resonate) : ViewModel() {
    private val features = resonate.features

    val book = features.providerAccounts
    val hostedServices = features.hostedServices
    val stallDownloadsOnMetered = features.stallDownloadsOnMetered

    fun turn(id: Long, on: Boolean) = features.turnProviderAccount(id, on)

    fun setHostedServices(enabled: Boolean) = features.setHostedServices(enabled)

    fun setStallDownloadsOnMetered(enabled: Boolean) =
        features.setStallDownloadsOnMetered(enabled)
}

@Composable
fun ProvidersScreen() {
    val model = resonateViewModel { ProvidersViewModel(it) }
    val navigator = LocalNavigator.current
    val book by model.book.collectAsStateWithLifecycle()
    val hostedServices by model.hostedServices.collectAsStateWithLifecycle()
    val stallDownloadsOnMetered by model.stallDownloadsOnMetered.collectAsStateWithLifecycle()
    Page("Download sources") { padding ->
        val all = book
        if (all == null) {
            Loading(padding)
            return@Page
        }
        SettingsList(padding, Modifier.testTag("providers-list")) {
            item(key = "intro") {
                Note(
                    "Songs you find in Search can be downloaded from the sources chosen here. Downloads are matched by recording codes, or by title, artist and length, and you choose the folder for each song. ${accountsLead(all.asked.size)}"
                )
            }
            item(key = "hosted-services") {
                SwitchRow(
                    title = "Ask the hosted services",
                    icon = Icons.Rounded.Cloud,
                    supporting =
                        "A Monochrome or hifi-api source with no server address is asked at the hosted service unless this is off",
                    checked = hostedServices,
                    onCheckedChange = model::setHostedServices,
                    modifier = Modifier.testTag("setting-hosted-services"),
                )
            }
            item(key = "stall-downloads-on-metered") {
                SwitchRow(
                    title = "Wait on a metered connection",
                    icon = Icons.Rounded.SignalCellularAlt,
                    supporting = "Downloads pause until the network is not metered",
                    checked = stallDownloadsOnMetered,
                    onCheckedChange = model::setStallDownloadsOnMetered,
                    modifier = Modifier.testTag("setting-stall-downloads-on-metered"),
                )
            }
            item(key = "sources-header") { SectionHeader("Your sources") }
            if (all.kept.isEmpty()) {
                item(key = "none") { Note("Add a source below to download songs.") }
            }
            all.kept.forEach { kept ->
                item(key = "account-${kept.id}") {
                    SettingRow(
                        title = providerName(kept.account.provider),
                        icon = iconFor(kept.account.provider),
                        supporting = accountSummary(kept.account, kept.on),
                        trailing = {
                            Switch(
                                checked = kept.on,
                                onCheckedChange = { model.turn(kept.id, it) },
                                modifier = Modifier.testTag("provider-on-${kept.id}"),
                            )
                        },
                        onClick = {
                            navigator.open(
                                Screen.ProviderAccount(kept.id, kept.account.provider.name)
                            )
                        },
                        modifier = Modifier.testTag("provider-account-${kept.id}"),
                    )
                }
            }
            item(key = "add-header") { SectionHeader("Add a source") }
            providersShown.filter(all::accepts).forEach { provider ->
                item(key = "add-${provider.name}") {
                    SettingRow(
                        title = providerName(provider),
                        icon = Icons.Rounded.Add,
                        supporting = providerDetail(provider),
                        onClick = {
                            navigator.open(
                                Screen.ProviderAccount(ProviderAccountBook.UNSAVED, provider.name)
                            )
                        },
                        modifier = Modifier.testTag("add-${provider.name.lowercase()}"),
                    )
                }
            }
            item(key = "keystore") {
                Footnote("Passwords, secrets and tokens stay encrypted with Android Keystore.")
            }
        }
    }
}

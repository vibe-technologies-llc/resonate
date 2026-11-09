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
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ProviderAccountBook
import org.resonate.android.R
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

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
    val words = words()
    Page(stringResource(R.string.providers_title)) { padding ->
        val all = book
        if (all == null) {
            Loading(padding)
            return@Page
        }
        SettingsList(padding, Modifier.testTag("providers-list")) {
            item(key = "intro") {
                Note(
                    stringResource(R.string.providers_intro, accountsLead(words, all.asked.size))
                )
            }
            item(key = "hosted-services") {
                SwitchRow(
                    title = stringResource(R.string.providers_hosted_services),
                    icon = Icons.Rounded.Cloud,
                    supporting = stringResource(R.string.providers_hosted_services_detail),
                    checked = hostedServices,
                    onCheckedChange = model::setHostedServices,
                    modifier = Modifier.testTag("setting-hosted-services"),
                )
            }
            item(key = "stall-downloads-on-metered") {
                SwitchRow(
                    title = stringResource(R.string.providers_stall_on_metered),
                    icon = Icons.Rounded.SignalCellularAlt,
                    supporting = stringResource(R.string.providers_stall_on_metered_detail),
                    checked = stallDownloadsOnMetered,
                    onCheckedChange = model::setStallDownloadsOnMetered,
                    modifier = Modifier.testTag("setting-stall-downloads-on-metered"),
                )
            }
            item(key = "sources-header") { SectionHeader(stringResource(R.string.providers_yours)) }
            if (all.kept.isEmpty()) {
                item(key = "none") { Note(stringResource(R.string.providers_none)) }
            }
            all.kept.forEach { kept ->
                item(key = "account-${kept.id}") {
                    SettingRow(
                        title = providerName(kept.account.provider),
                        icon = iconFor(kept.account.provider),
                        supporting = accountSummary(words, kept.account, kept.on),
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
            item(key = "add-header") { SectionHeader(stringResource(R.string.providers_add)) }
            providersShown.filter(all::accepts).forEach { provider ->
                item(key = "add-${provider.name}") {
                    SettingRow(
                        title = providerName(provider),
                        icon = Icons.Rounded.Add,
                        supporting = providerDetail(words, provider),
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
                Footnote(stringResource(R.string.provider_keystore))
            }
        }
    }
}

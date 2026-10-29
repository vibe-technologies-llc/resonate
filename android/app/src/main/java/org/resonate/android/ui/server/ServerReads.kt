package org.resonate.android.ui.server

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.library.Loaded
import org.resonate.android.ui.library.Retry

suspend fun <T> Resonate.asked(load: (MobilePlayer) -> T): Result<T> {
    val account = features.served.account.value
    return read { native ->
        native.serveFrom(account)
        withContext(Dispatchers.IO) { load(native) }
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
fun <T> Resonate.servedRead(retry: Retry, load: (MobilePlayer) -> T): Flow<Loaded<T>> =
    combine(features.served.account, retry.asked) { account, _ -> account }
        .flatMapLatest {
            flow {
                emit(Loaded.Loading)
                asked(load)
                    .onSuccess { emit(Loaded.Ready(it)) }
                    .onFailure { failure ->
                        library.report(failure as? Exception ?: IllegalStateException(failure))
                        emit(Loaded.Failed)
                    }
            }
        }

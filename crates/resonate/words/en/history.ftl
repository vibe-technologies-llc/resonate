exported = { $count ->
    [one] wrote one listen to { $file }
   *[other] wrote { $count } listens to { $file }
}
brought-so-far = { $credited } counted, { $held } held already, { $unmatched } of songs not held here
brought-in = brought in { $credited } from { $service }: { $held ->
    [one] { $held } was held already
   *[other] { $held } were held already
}, and { $unmatched ->
    [one] { $unmatched } names no song held here, which a later run brings in once it is
   *[other] { $unmatched } name no song held here, which a later run brings in once it is
}
brought-in-from-file = brought in { $credited } from { $file }: { $held ->
    [one] { $held } was held already
   *[other] { $held } were held already
}, and { $unmatched ->
    [one] { $unmatched } names no song held here, which a later run brings in once it is
   *[other] { $unmatched } name no song held here, which a later run brings in once it is
}

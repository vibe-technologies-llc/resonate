on = { $title } sur { $album }
wanted = recherché { $shown }
dismissed = rejeté { $shown }
tracks-dismissed = { $tracks ->
    [one] { $tracks } Voie manquante
   *[other] { $tracks } voies manquantes
} --bring-back les énumère à nouveau
dismissed-by = { $title } par { $artist }
releases-dismissed = { $releases ->
    [one] { $releases } libération
   *[other] { $releases } rejets
} --bring-back les énumère à nouveau
no-longer-wanted = ne veut plus: { $shown }

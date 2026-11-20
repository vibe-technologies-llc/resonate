written = { $count ->
    [one] Enregistré les paroles d’une chanson à côté de son fichier
   *[other] Enregistré les paroles de { $count } chansons à côté de leurs fichiers
}
already-beside = { $count ->
    [one] Une chanson avait déjà ces paroles à côté.
   *[other] { $count } chansons avaient déjà ces paroles à côté d’eux
}
left-alone = { $count ->
    [one] Une feuille lyrique se tenait déjà à côté d’une chanson, donc elle était laissée seule : choisissez Sur une feuille déjà là pour la remplacer
   *[other] Des feuilles lyriques se tenaient déjà à côté de { $count } chansons, donc elles étaient laissées seules : choisissez Sur une feuille déjà là pour les remplacer
}
outranked = { $count ->
    [one] Une feuille lyrique plus riche à côté d’une chanson est lue avant ces paroles, donc rien n’a été écrit pour elle
   *[other] Richer feuilles lyriques à côté { $count } chansons sont lues avant ces paroles, donc rien n’a été écrit pour eux
}
none-kept = { $count ->
    [one] Pas de paroles récupérées sont gardées pour une chanson
   *[other] Aucune paroles récupérées ne sont conservées pour { $count } chansons
}
no-file-of-its-own = { $count ->
    [one] Une chanson est coupée d’un fichier partagé ou conservée dans le coffre, donc il n’a pas de feuille de son propre lyrique
   *[other] { $count } chansons sont coupées d’un fichier partagé ou conservées dans la chambre forte, donc ils n’ont pas de feuille lyrique de leur propre
}
not-written = { $count ->
    [one] Les paroles d’une chanson ne pouvaient pas être sauvegardées à côté
   *[other] Les paroles de { $count } chansons ne pouvaient pas être sauvées à côté d’eux
}

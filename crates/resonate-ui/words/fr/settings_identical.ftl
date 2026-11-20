identical-songs = Chansons identiques
page-about = Fichiers contenant les mêmes octets, pliés dans la chanson qu’ils copient
hint = Il trouve des fichiers contenant exactement les mêmes octets, comme Song (1).flac à côté de Song.flac ou une chanson copiée dans un autre dossier sous un nouveau nom. De chaque ensemble, le fichier non numéroté comme une copie, et parmi ceux qui sont catalogués en premier, est la chanson; les autres sont ses copies. Pliez les pièces d’une copie, les entrées favorites, les listes de lecture et les lignes de file d’attente à cette chanson et envoyez la copie à la corbeille. Un fichier numéroté dont les octets diffèrent de tous les autres est laissé là où il est. Rien n’est replié seul: Trouver des listes ce qui serait, et seul Fold change quoi que ce soit.
also-called = duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata dulic duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplicata duplia duplia duplicata duplicata duplicata duplia duplia duplia duplia duplia duplia duplicata duplica duplia duplica duplia duplia duplia duplia duplia duplia duplia duplia duplia duplia duplia duplia numéroa duplia duplia numéroa duplia duplia duplia dua dua dua dua dua dua dua dua dua dua dua dua dua dua dua dua dua dua dua duplia du
find = Rechercher
finding = Trouver...
fold = Plier
folding = Plié...
press-again-to-fold = Appuyez à nouveau sur pour plier
find-first = Trouver d’abord, donc les chansons tenues deux fois sont à l’écran avant que quelque chose ne soit plié.
moved-under-the-search = Le catalogue a déménagé depuis la dernière recherche, donc il a été enlevé. Trouver à nouveau pour voir ce qui serait plié maintenant.
ask-again-to-fold = Appuyez encore une fois pour plier les copies. Le fichier de chaque copie va à la poubelle et ses pièces, les entrées favorites, les listes de lecture et les lignes de file d’attente vont à la chanson qu’elle copie.
none-held-twice = Aucun fichier ne contient les mêmes octets.
would-fold = { $copies ->
    [one] plierait { $copies } copie
   *[other] plierait { $copies } copies
} dans { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
} · { $differing ->
    [one] { $differing } fichier numéroté diffère et reste
   *[other] { $differing } fichiers numérotés diffèrent et restent
} · rien n’a été plié
copies-of-it = { $count ->
    [one] Reçu : { $names }
   *[other] { $count } exemplaires : { $names }
}
and-more = et { $rest } plus
note = Seuls les fichiers contenant exactement les mêmes octets sont des copies les uns des autres. Trouvez les listes; seul Fold envoie une copie à la poubelle, et la chanson garde tout ce que la copie avait.

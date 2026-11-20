no-folder-title = Choisissez d’abord un dossier musical
no-folder-says = Les chansons déposées ici sont copiées dans votre dossier musical principal. Définissez-en un dans Paramètres sous Bibliothèque.
folder-gone-title = Le dossier de musique n’est pas là
folder-gone-says = S’il est sur un lecteur, monter, puis déposer à nouveau.
nothing-title = Rien ici que la bibliothèque lit
nothing-says = Les fichiers audio, les feuilles de repère et les dossiers les tenant sont copiés, avec les couvertures et les paroles à côté d’eux; tout ce qui reste est là où il est.
busy-title = Encore en copie
busy-says = Déposer encore une fois une fois que les chansons copiées ont atterri.
ready-says = Copiés comme ils sont dans
ready-to-file-says = Copié et déposé par votre mise en page dans
weighing-title = En regardant ce qui est traîné...
weighing-says = Déposer quand il dit ce qui serait copié.
stopped-after-copying = { $count ->
    [one] Arrêt après copie du fichier { $count }
   *[other] Arrêt après copie des fichiers { $count }
}
copied-into = { $count ->
    [one] Copie du fichier { $count }
   *[other] Fichiers copiés { $count }
} ({ $bytes }) vers { $folder }
copied-into-beside-some-there = { $count ->
    [one] Copie du fichier { $count }
   *[other] Fichiers copiés { $count }
} ({ $bytes }) dans { $folder } · { $held } déjà là
already-in = Déjà dans { $folder }, donc rien n’a été copié
nothing-copied-source-gone = Rien n’a été copié — un fichier n’est plus là
nothing-copied-unreadable = Rien n’a été copié — un fichier n’a pu être lu
nothing-copied-not-audio = Rien n’a été copié — un fichier n’est pas un format que lit la bibliothèque
nothing-copied-nothing-inside = Rien n’a été copié — un fichier ne contient aucun audio
nothing-copied-already-there = Rien n’a été copié — un fichier est déjà dans le dossier
nothing-copied-already-held = Rien n’a été copié — un fichier est déjà dans le dossier, octet pour octet
nothing-copied-unwritable = Rien n’a été copié — un fichier ne pouvait pas être écrit dans le dossier
nothing-copied-unverified = Rien n’a été copié — un fichier n’a pas été lu de la même façon qu’il a été envoyé
looks-audio = copié
looks-sheet = feuille de queue, copiée
looks-companion = à côté d’une chanson, copiée avec
looks-folder = dossier, son audio, couvertures et paroles sont copiés
looks-other = Pas audio, laissés de côté
looks-gone = pas là
copy-the-songs = copier les chansons
finish-copying-the-songs = finir de copier les chansons
drop-to-add = { $count ->
    [one] Déposer pour ajouter { $count } élément
   *[other] Déposer pour ajouter des éléments { $count }
}
ready-and-left-out = { $ready } · { $count ->
    [one] { $count } poste
   *[other] { $count } points de l ’ ordre du jour
} laissés à l’écart
and-more = et { $count } plus
stopping = Arrêter...
copying = Copie { $reached } de { $found } · { $copied } de { $whole }
stop-copying = Arrêter de copier

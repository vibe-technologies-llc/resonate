exported = { $count ->
    [one] { $file } a écrit une écoute.
   *[other] écrit { $count } écoute { $file }
}
brought-so-far = { $credited ->
    [one] { $credited } comptée
   *[other] { $credited } comptées
}, { $held ->
    [one] { $held } déjà connue
   *[other] { $held } déjà connues
}, { $unmatched ->
    [one] { $unmatched } sans morceau ici
   *[other] { $unmatched } sans morceau ici
}
brought-in = { $credited ->
    [one] { $credited } écoute importée
   *[other] { $credited } écoutes importées
} de { $service } : { $held ->
    [one] { $held } figurait déjà dans le catalogue
   *[other] { $held } figuraient déjà dans le catalogue
}, et { $unmatched ->
    [one] { $unmatched } ne correspond à aucun morceau d’ici, qu’une importation ultérieure reprendra dès qu’il y sera
   *[other] { $unmatched } ne correspondent à aucun morceau d’ici, qu’une importation ultérieure reprendra dès qu’ils y seront
}
brought-in-from-file = { $credited ->
    [one] { $credited } écoute importée
   *[other] { $credited } écoutes importées
} de { $file } : { $held ->
    [one] { $held } figurait déjà dans le catalogue
   *[other] { $held } figuraient déjà dans le catalogue
}, et { $unmatched ->
    [one] { $unmatched } ne correspond à aucun morceau d’ici, qu’une importation ultérieure reprendra dès qu’il y sera
   *[other] { $unmatched } ne correspondent à aucun morceau d’ici, qu’une importation ultérieure reprendra dès qu’ils y seront
}

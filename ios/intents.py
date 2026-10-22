import json
from pathlib import Path
import shutil

TOOLS_VERSION = "27A266a"
MODULE = "Resonate"
STRING = 0
INTEGER = 2
WILDCARD = {"LNPlatformNameWildcard": {"introducedVersion": "*"}}
SYSTEM_PROTOCOLS = {"AppIntents.AudioPlaybackIntent": "com.apple.link.systemProtocol.AudioStarting"}
UNREMARKABLE = {
    "AppIntents.AppIntent",
    "AppIntents.PersistentlyIdentifiable",
    "AppIntents._SupportsAppDependencies",
    "AppIntents.SystemIntent",
}
OUTPUT_FLAGS = {"AppIntents.IntentResult": 0, "AppIntents.ProvidesDialog": 4}
UNSUPPORTED_KINDS = ("AppIntents.AppEntity", "AppIntents.AppEnum", "AppIntents.EntityQuery")
PARAMETER_ARGUMENTS = {"title", "description", "default", "inputOptions", "requestValueDialog", "inputConnectionBehavior"}
WRITTEN_ANYWAY = {"requestValueDialog", "inputConnectionBehavior"}


class Unsupported(SystemExit):
    def __init__(self, what):
        super().__init__(f"app-intents.py: {what} is not supported; Siri would not match it to the binary")


def text(key):
    return {"alternatives": [], "key": key}


def primitive(type_id):
    return {"primitive": {"wrapper": {"typeIdentifier": type_id}}}


def static(entry, label):
    return next((p for p in entry.get("properties", []) if p["label"] == label and p["isStatic"] == "true"), None)


def literal(value, what):
    if value is None:
        raise Unsupported(f"{what} without a literal")
    if value.get("valueKind") == "RawLiteral":
        return value["value"]
    if value.get("valueKind") == "InitCall":
        arguments = value["value"]["arguments"]
        if arguments and arguments[0].get("valueKind") == "RawLiteral" and all(
            extra.get("valueKind") == "NilLiteral" for extra in arguments[1:]
        ):
            return arguments[0]["value"]
    raise Unsupported(f"{what} as {value.get('valueKind')}")


def short_name(type_name):
    return type_name.rsplit(".", 1)[-1]


def wrapped(type_name, wrapper):
    prefix = wrapper + "<"
    return type_name[len(prefix):-1] if type_name.startswith(prefix) and type_name.endswith(">") else None


def parameter(prop, intent):
    name = prop["label"][1:]
    what = f"{intent}.{name}"
    value_type = wrapped(prop["type"], "AppIntents.IntentParameter")
    optional = wrapped(value_type, "Swift.Optional")
    if (optional or value_type) != "Swift.String" or prop.get("valueKind") != "InitCall":
        raise Unsupported(f"{what} of {value_type}")

    arguments = {argument["label"]: argument for argument in prop["value"]["arguments"]}
    for label, argument in arguments.items():
        if label not in PARAMETER_ARGUMENTS or (label not in WRITTEN_ANYWAY | {"title"} and argument.get("valueKind") != "NilLiteral"):
            raise Unsupported(f"{what}'s {label}")

    return {
        "capabilities": 0,
        "dynamicOptionsSupport": 0,
        "inputConnectionBehavior": 0,
        "isInput": False,
        "isOptional": optional is not None,
        "name": name,
        "resolvableInputTypes": [
            {"kindValue": 0, "valueType": {"array": {"wrapper": {"capabilities": 3, "memberValueType": primitive(INTEGER)}}}},
            {"kindValue": 0, "valueType": primitive(STRING)},
            {"kindValue": 0, "valueType": primitive(INTEGER)},
        ],
        "title": text(literal(arguments.get("title"), f"{what}'s title")),
        "typeSpecificMetadata": [],
        "valueType": primitive(STRING),
    }


def output_flags(entry, name):
    result = next(
        (alias["substitutedTypeName"] for alias in entry.get("associatedTypeAliases", []) if alias["typeAliasName"] == "PerformResult"),
        None,
    )
    if not (result or "").startswith("some "):
        raise Unsupported(f"{name}.perform() returning {result}")
    parts = result[len("some "):].split(" & ")
    unknown = [part for part in parts if part not in OUTPUT_FLAGS]
    if unknown:
        raise Unsupported(f"{name}.perform() returning {' & '.join(unknown)}")
    return sum(OUTPUT_FLAGS[part] for part in parts)


def action(entry):
    name = entry["typeName"]
    conformances = entry["conformances"]
    foreign = [c for c in conformances if c.startswith("AppIntents.") and c not in UNREMARKABLE and c not in SYSTEM_PROTOCOLS]
    if foreign:
        raise Unsupported(f"{name}'s {', '.join(foreign)}")
    for label in ("parameterSummary", "supportedModes", "allowedExecutionTargets", "isDiscoverable", "authenticationPolicy"):
        if static(entry, label) is not None:
            raise Unsupported(f"{name}.{label}")

    opens = static(entry, "openAppWhenRun")
    opens_app = literal(opens, f"{name}.openAppWhenRun") == "true" if opens is not None else False
    protocols = [SYSTEM_PROTOCOLS[c] for c in conformances if c in SYSTEM_PROTOCOLS]
    protocol_metadata = [part for protocol in protocols for part in (protocol, {"empty": {}})]
    parameters = [
        parameter(prop, name)
        for prop in entry.get("properties", [])
        if prop["label"].startswith("_") and prop["type"].startswith("AppIntents.IntentParameter<")
    ]

    described = {
        "assistantDefinedSchemaTraits": [],
        "assistantDefinedSchemas": [],
        "authenticationPolicy": 0,
        "availabilityAnnotations": WILDCARD,
        "effectiveBundleIdentifiers": [],
        "fullyQualifiedTypeName": name,
        "identifier": short_name(name),
        "isAuthPolExplicit": False,
        "isDiscoverable": True,
        "mangledTypeName": entry["mangledTypeName"],
        "mangledTypeNameByBundleIdentifier": {},
        "mangledTypeNameByBundleIdentifierV2": {},
        "mangledTypeNameV2": entry["mangledTypeName"],
        "openAppWhenRun": opens_app,
        "outputFlags": output_flags(entry, name),
        "parameters": parameters,
        "presentationStyle": 0,
        "requiredCapabilities": [],
        "supportedModes": 2 if opens_app else 1,
        "systemProtocolMetadata": protocol_metadata,
        "systemProtocolMetadataV2": protocol_metadata,
        "systemProtocols": protocols,
        "title": text(literal(static(entry, "title"), f"{name}.title")),
        "typeSpecificMetadata": [],
        "visibilityMetadata": {"assistantOnly": False, "isDiscoverable": True},
    }
    description = static(entry, "description")
    if description is not None:
        described["descriptionMetadata"] = {"descriptionText": text(literal(description, f"{name}.description")), "searchKeywords": []}
    return described


def phrase(value, provider):
    if value.get("valueKind") == "RawLiteral":
        raise Unsupported(f"{provider}: a phrase without the app's name")
    if value.get("valueKind") != "InterpolatedStringLiteral":
        raise Unsupported(f"{provider}: a phrase as {value.get('valueKind')}")
    spoken = ""
    for segment in value["value"]["segments"]:
        if segment.get("valueKind") == "RawLiteral":
            spoken += segment["value"]
        elif segment.get("valueKind") == "Enum" and segment["value"]["name"] == "applicationName":
            spoken += "${applicationName}"
        else:
            raise Unsupported(f"{provider}: a phrase interpolating anything but .applicationName")
    return text(spoken)


def shortcuts(entry, actions):
    provider = entry["typeName"]
    built = static(entry, "appShortcuts")
    if built is None or built.get("valueKind") != "Builder":
        raise Unsupported(f"{provider}.appShortcuts other than a list of AppShortcut(...)")

    listed = []
    for member in built["value"]["members"]:
        element = member.get("element", {})
        if member.get("kind") != "buildExpression" or element.get("valueKind") != "InitCall" or element["value"]["type"] != "AppIntents.AppShortcut":
            raise Unsupported(f"{provider}.appShortcuts holding {member.get('kind')}")
        arguments = {argument["label"]: argument for argument in element["value"]["arguments"]}
        if set(arguments) != {"intent", "phrases", "shortTitle", "systemImageName"}:
            raise Unsupported(f"{provider}: an AppShortcut with {sorted(arguments)}")
        intent = arguments["intent"]
        if intent.get("valueKind") != "InitCall" or intent["value"]["arguments"]:
            raise Unsupported(f"{provider}: an intent made with arguments")
        identifier = short_name(intent["value"]["type"])
        if identifier not in actions:
            raise Unsupported(f"{provider}: {identifier}, which is no intent of {MODULE}")

        listed.append(
            {
                "actionIdentifier": identifier,
                "availabilityAnnotations": WILDCARD,
                "phraseTemplates": [phrase(p, provider) for p in arguments["phrases"]["value"]],
                "shortTitle": text(literal(arguments["shortTitle"], f"{provider}'s shortTitle")),
                "systemImageName": literal(arguments["systemImageName"], f"{provider}'s systemImageName"),
            }
        )
    return listed


def metadata(entries):
    for entry in entries:
        kinds = [kind for kind in UNSUPPORTED_KINDS if kind in entry["conformances"]]
        if kinds:
            raise Unsupported(f"{entry['typeName']} as {', '.join(kinds)}")

    actions = {}
    for entry in entries:
        if "AppIntents.AppIntent" in entry["conformances"]:
            described = action(entry)
            actions[described["identifier"]] = described
    providers = [entry for entry in entries if "AppIntents.AppShortcutsProvider" in entry["conformances"]]
    if len(providers) > 1:
        raise Unsupported("more than one AppShortcutsProvider")

    written = {
        "actions": actions,
        "assistantEntities": [],
        "assistantIntentNegativePhrases": [],
        "assistantIntents": [],
        "autoShortcuts": shortcuts(providers[0], actions) if providers else [],
        "entities": {},
        "enums": [],
        "generator": {"name": "xcode-tools", "version": TOOLS_VERSION},
        "negativePhrases": [],
        "queries": {},
        "shortcutTileColor": 14,
        "version": 1,
    }
    if providers:
        written["autoShortcutProviderMangledName"] = providers[0]["mangledTypeName"]
    return written


def write(into, written):
    if into.exists():
        shutil.rmtree(into)
    into.mkdir()
    (into / "extract.actionsdata").write_text(json.dumps(written, sort_keys=True, separators=(",", ":"), ensure_ascii=False))
    (into / "version.json").write_text('{\n  "toolsVersion" : "%s",\n  "version" : "3.0"\n}' % TOOLS_VERSION)

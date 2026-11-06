"""Optional installed-IntelliJ acceptance gate; never writes global IDE configuration."""
import pathlib
import tempfile
import xml.etree.ElementTree as ET
from test_project import ROOT, run

def smoke(project, idea):
    project = pathlib.Path(project)
    configs = sorted((project / ".idea/runConfigurations").glob("*.xml"))
    config = ET.parse(configs[0]).getroot().find("configuration")
    options = {o.attrib["name"]: o.attrib.get("value") for o in config.findall("option")}
    jdk = options["ALTERNATIVE_JRE_PATH"]
    main = options["MAIN_CLASS_NAME"]
    module = config.find("module").attrib["name"]
    cp = f"{idea}/lib/*:{idea}/plugins/java/lib/*"
    with tempfile.TemporaryDirectory(prefix="jix-jps-") as temp:
        run(f"{jdk}/bin/javac", "-cp", cp, "-d", temp, ROOT / "tools/idea/JpsSmoke.java")
        result = run(f"{jdk}/bin/java", f"-Didea.home.path={idea}", "-cp", f"{temp}:{cp}", "JpsSmoke", project, jdk, module, main)
        assert "source breakpoint passed" in result.stdout, result.stdout
    print("IntelliJ JPS build, native launch and source breakpoint passed", flush=True)

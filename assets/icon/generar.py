#!/usr/bin/env python3
"""Genera los íconos derivados desde konstruado.svg (la única fuente, hecha a mano).

Salidas:
  assets/icon/png/konstruado-{16..512}.png          PNG cuadrados (escritorio, docs)
  assets/linux/konstruado.png                         256 px para el .desktop
  crates/konstruado/assets/konstruado-256.png         ícono de ventana (include_bytes!)
  android/app/src/main/res/drawable/ic_launcher_foreground.xml   adaptive (vector)
  android/app/src/main/res/drawable/ic_launcher_monochrome.xml   íconos temáticos (A13+)
  android/app/src/main/res/values/ic_launcher_background.xml     color de fondo
  android/app/src/main/res/mipmap-*/ic_launcher{,_round}.png     respaldo en PNG

Requiere rsvg-convert (librsvg2-bin). Uso: python3 assets/icon/generar.py
"""
import os
import re
import subprocess
import tempfile
import xml.etree.ElementTree as ET

AQUI = os.path.dirname(os.path.abspath(__file__))
RAIZ = os.path.dirname(os.path.dirname(AQUI))
SVG = os.path.join(AQUI, "konstruado.svg")
NS = {"s": "http://www.w3.org/2000/svg"}
FONDO = "#b8321f"
# Arte del primer plano del adaptive icon: entra en el círculo seguro de 66/108 dp.
ESCALA_ADAPTIVE = 0.64


def rect_a_path(x, y, w, h, r):
    r = min(r, w / 2, h / 2)
    return (f"M{x + r:g},{y:g}H{x + w - r:g}A{r:g},{r:g} 0 0 1 {x + w:g},{y + r:g}"
            f"V{y + h - r:g}A{r:g},{r:g} 0 0 1 {x + w - r:g},{y + h:g}"
            f"H{x + r:g}A{r:g},{r:g} 0 0 1 {x:g},{y + h - r:g}"
            f"V{y + r:g}A{r:g},{r:g} 0 0 1 {x + r:g},{y:g}Z")


def forma_a_path(el):
    tag = el.tag.split("}")[1]
    if tag == "path":
        return el.get("d")
    if tag == "rect":
        f = lambda k, d=0: float(el.get(k, d))
        return rect_a_path(f("x"), f("y"), f("width"), f("height"), f("rx"))
    raise ValueError(tag)


def leer_arte():
    """Devuelve [(rotación, borde, [(path, color)])] del grupo #arte, y su transform."""
    raiz = ET.parse(SVG).getroot()
    arte = raiz.find(".//s:g[@id='arte']", NS)
    m = re.match(r"translate\(256 (\d+)\) scale\(([\d.]+)\)", arte.get("transform"))
    dy, escala = float(m.group(1)) - 256, float(m.group(2))
    grupos = []
    for g in arte.findall("s:g", NS):
        rot = float(re.match(r"rotate\((-?[\d.]+)", g.get("transform")).group(1))
        # Un subgrupo con stroke = borde del color de fondo (separa la pala de la picota).
        sub = g.find("s:g", NS)
        borde = float(sub.get("stroke-width")) if sub is not None else 0.0
        formas = [(forma_a_path(e), e.get("fill")) for e in g if not e.tag.endswith("}g")]
        grupos.append((rot, borde, formas))
    return grupos, dy, escala


def vector(grupos, dy, escala, mono):
    out = ['<?xml version="1.0" encoding="utf-8"?>',
           "<!-- Generado por assets/icon/generar.py desde assets/icon/konstruado.svg. No editar. -->",
           '<vector xmlns:android="http://schemas.android.com/apk/res/android"',
           '    android:width="108dp" android:height="108dp"',
           '    android:viewportWidth="512" android:viewportHeight="512">',
           f'  <group android:pivotX="256" android:pivotY="256" android:scaleX="{escala:g}"'
           f' android:scaleY="{escala:g}" android:translateY="{dy * escala / 0.84:g}">']
    for rot, borde, formas in grupos:
        out.append(f'    <group android:pivotX="256" android:pivotY="256" android:rotation="{rot:g}">')
        if borde and not mono:
            # Borde del color de fondo: separa la pala de la picota (paint-order=stroke).
            for d, _ in formas:
                out.append(f'      <path android:pathData="{d}" android:fillColor="{FONDO}"'
                           f' android:strokeColor="{FONDO}" android:strokeWidth="{borde:g}"'
                           ' android:strokeLineJoin="round"/>')
        for d, color in formas:
            c = "#FFFFFFFF" if mono else color
            out.append(f'      <path android:pathData="{d}" android:fillColor="{c}"/>')
        out.append("    </group>")
    out += ["  </group>", "</vector>", ""]
    return "\n".join(out)


def svg_variante(fondo_circular=False, sin_fondo=False, escala=None):
    s = open(SVG).read()
    if escala is not None:
        s = re.sub(r'translate\(256 (\d+)\) scale\(([\d.]+)\)',
                   lambda m: f"translate(256 {256 + (float(m.group(1)) - 256) * escala / 0.84:g}) scale({escala:g})", s)
    if sin_fondo:
        s = re.sub(r'<rect width="512" height="512" rx="112" fill="[^"]+"/>', "", s)
    elif fondo_circular:
        s = re.sub(r'<rect width="512" height="512" rx="112"', '<rect width="512" height="512" rx="256"', s)
    return s


def png(svg_texto, lado, destino):
    os.makedirs(os.path.dirname(destino), exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as t:
        t.write(svg_texto)
    subprocess.run(["rsvg-convert", "-w", str(lado), "-h", str(lado), t.name, "-o", destino], check=True)
    os.unlink(t.name)


def escribir(ruta, texto):
    os.makedirs(os.path.dirname(ruta), exist_ok=True)
    with open(ruta, "w") as f:
        f.write(texto)


def main():
    base = open(SVG).read()
    for lado in (16, 24, 32, 48, 64, 128, 256, 512):
        png(base, lado, os.path.join(AQUI, "png", f"konstruado-{lado}.png"))
    png(base, 256, os.path.join(RAIZ, "assets", "linux", "konstruado.png"))
    png(base, 256, os.path.join(RAIZ, "crates", "konstruado", "assets", "konstruado-256.png"))

    res = os.path.join(RAIZ, "android", "app", "src", "main", "res")
    grupos, dy, _ = leer_arte()
    escribir(os.path.join(res, "drawable", "ic_launcher_foreground.xml"),
             vector(grupos, dy, ESCALA_ADAPTIVE, mono=False))
    escribir(os.path.join(res, "drawable", "ic_launcher_monochrome.xml"),
             vector(grupos, dy, ESCALA_ADAPTIVE, mono=True))
    escribir(os.path.join(res, "values", "ic_launcher_background.xml"),
             '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n'
             f'    <color name="ic_launcher_background">{FONDO}</color>\n</resources>\n')
    adaptive = ('<?xml version="1.0" encoding="utf-8"?>\n'
                '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
                '    <background android:drawable="@color/ic_launcher_background"/>\n'
                '    <foreground android:drawable="@drawable/ic_launcher_foreground"/>\n'
                '    <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>\n'
                '</adaptive-icon>\n')
    for nombre in ("ic_launcher", "ic_launcher_round"):
        escribir(os.path.join(res, "mipmap-anydpi-v26", f"{nombre}.xml"), adaptive)
    # PNG de respaldo (launchers sin adaptive): cuadrado redondeado y redondo.
    for carpeta, lado in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        png(base, lado, os.path.join(res, f"mipmap-{carpeta}", "ic_launcher.png"))
        png(svg_variante(fondo_circular=True, escala=0.74), lado,
            os.path.join(res, f"mipmap-{carpeta}", "ic_launcher_round.png"))
    print("íconos generados")


if __name__ == "__main__":
    main()

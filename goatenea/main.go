// Atenea Go shell: all UI lives here (Fyne). The Rust engine is a
// sidecar: `acore <summary|tables|wires|sim> <file>` prints JSON.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"
)

var acoreBin = "acore"

func init() {
	if p, err := exec.LookPath("acore"); err == nil {
		acoreBin = p
	} else if home, err := os.UserHomeDir(); err == nil {
		cand := filepath.Join(home, ".local", "bin", "acore")
		if _, err := os.Stat(cand); err == nil {
			acoreBin = cand
		}
	}
}

func runAcore(cmd, file string, args ...string) ([]byte, error) {
	argv := append([]string{cmd, file}, args...)
	out, err := exec.Command(acoreBin, argv...).Output()
	if err != nil {
		if ee, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("acore: %s", string(ee.Stderr))
		}
		return nil, err
	}
	return out, nil
}

type summaryDoc struct {
	File    string            `json:"file"`
	Kind    string            `json:"kind"`
	Summary map[string]string `json:"summary"`
	Counts  map[string]int    `json:"counts"`
}

type partRow struct {
	Ref   string   `json:"ref"`
	Kind  string   `json:"kind"`
	Value string   `json:"value"`
	Rest  []string `json:"rest"`
}

type netRow struct {
	ID   string     `json:"id"`
	Pins [][]string `json:"pins"`
}

type tablesDoc struct {
	File  string    `json:"file"`
	Parts []partRow `json:"parts"`
	Nets  []netRow  `json:"nets"`
}

type state struct {
	w      fyne.Window
	status *widget.Label
	tabs   *container.AppTabs
}

func (s *state) setStatus(msg string) {
	s.status.SetText(msg)
}

func (s *state) openFile(path string) {
	out, err := runAcore("summary", path)
	if err != nil {
		s.setStatus("error: " + err.Error())
		return
	}
	var sum summaryDoc
	if err := json.Unmarshal(out, &sum); err != nil {
		s.setStatus("error: bad acore json")
		return
	}
	s.setStatus(fmt.Sprintf("%s · %s", filepath.Base(path), sum.Kind))
	s.showTables(path)
	s.showEsquema(path)
	s.showSimulate(path)
}

type wiresDoc struct {
	File       string      `json:"file"`
	Wires      [][]float64 `json:"wires"`
	Placements [][]any     `json:"placements"`
}

func (s *state) showEsquema(path string) {
	out, err := runAcore("wires", path)
	if err != nil {
		return // no wire geometry (MDF/IDX/pdsprj): leave placeholder
	}
	var doc wiresDoc
	if err := json.Unmarshal(out, &doc); err != nil || len(doc.Wires) == 0 {
		return
	}
	var x0, y0, x1, y1 float64
	for i, w := range doc.Wires {
		if len(w) < 4 {
			continue
		}
		xs := []float64{w[0], w[2]}
		ys := []float64{w[1], w[3]}
		if i == 0 {
			x0, x1, y0, y1 = xs[0], xs[1], ys[0], ys[1]
			if x0 > x1 {
				x0, x1 = x1, x0
			}
			if y0 > y1 {
				y0, y1 = y1, y0
			}
			continue
		}
		for _, x := range xs {
			if x < x0 {
				x0 = x
			}
			if x > x1 {
				x1 = x
			}
		}
		for _, y := range ys {
			if y < y0 {
				y0 = y
			}
			if y > y1 {
				y1 = y
			}
		}
	}
	const W, H = 1200.0, 800.0
	bw, bh := x1-x0, y1-y0
	if bw < 1e-9 {
		bw = 1
	}
	if bh < 1e-9 {
		bh = 1
	}
	sc := min(W/bw, H/bh) * 0.95
	if sc > 8 {
		sc = 8
	}
	mp := func(x, y float64) (float32, float32) {
		return float32((x-x0)*sc + 20), float32((y-y0)*sc + 20)
	}
	objs := []fyne.CanvasObject{}
	paper := canvas.NewRectangle(fyne.CurrentApp().Settings().Theme().Color("background", 0))
	paper.Resize(fyne.NewSize(float32(W+40), float32(H+40)))
	objs = append(objs, paper)
	for _, w := range doc.Wires {
		if len(w) < 5 {
			continue
		}
		x1s, y1s := mp(w[0], w[1])
		x2s, y2s := mp(w[2], w[3])
		ln := canvas.NewLine(fyne.CurrentApp().Settings().Theme().Color("foreground", 0))
		ln.Position1 = fyne.NewPos(x1s, y1s)
		ln.Position2 = fyne.NewPos(x2s, y2s)
		if w[4] != 0 {
			ln.StrokeWidth = 3
		} else {
			ln.StrokeWidth = 1
		}
		objs = append(objs, ln)
	}
	box := container.NewWithoutLayout(objs...)
	box.Resize(fyne.NewSize(float32(W+40), float32(H+40)))
	for _, tab := range s.tabs.Items {
		if tab.Text == "Esquema" {
			tab.Content = container.NewScroll(box)
			s.tabs.Refresh()
			return
		}
	}
}

type simDoc struct {
	Vars []string    `json:"vars"`
	Rows [][]float64 `json:"rows"`
}

func (s *state) showSimulate(path string) {
	out, err := runAcore("sim", path, "--vars", "v(OUT)", "--tstop", "5m")
	if err != nil {
		for _, tab := range s.tabs.Items {
			if tab.Text == "Simulate" {
				tab.Content = widget.NewLabel("sin ngspice o sin SDF: " + err.Error())
				s.tabs.Refresh()
				return
			}
		}
		return
	}
	var doc simDoc
	if err := json.Unmarshal(out, &doc); err != nil || len(doc.Rows) == 0 {
		return
	}
	rows := [][]string{append([]string{"t"}, doc.Vars...)}
	for _, r := range doc.Rows {
		row := make([]string, len(r))
		for i, v := range r {
			row[i] = fmt.Sprintf("%.6g", v)
		}
		rows = append(rows, row)
	}
	t := widget.NewTable(
		func() (int, int) { return len(rows), len(rows[0]) },
		func() fyne.CanvasObject { return widget.NewLabel("") },
		func(id widget.TableCellID, o fyne.CanvasObject) {
			o.(*widget.Label).SetText(rows[id.Row][id.Col])
		},
	)
	for _, tab := range s.tabs.Items {
		if tab.Text == "Simulate" {
			title := widget.NewLabel(fmt.Sprintf("%d filas", len(doc.Rows)))
			tab.Content = container.NewBorder(title, nil, nil, nil, container.NewScroll(t))
			s.tabs.Refresh()
			return
		}
	}
}

func (s *state) showTables(path string) {
	out, err := runAcore("tables", path)
	if err != nil {
		// DSN/LYT/pdsprj have no part tables: show summary instead.
		sout, serr := runAcore("summary", path)
		if serr != nil {
			s.setStatus("error: " + err.Error())
			return
		}
		var sum summaryDoc
		_ = json.Unmarshal(sout, &sum)
		rows := [][]string{{"campo", "valor"}}
		for k, v := range sum.Summary {
			if k == "_" {
				continue
			}
			rows = append(rows, []string{k, v})
		}
		for k, v := range sum.Counts {
			rows = append(rows, []string{k, fmt.Sprint(v)})
		}
		t := widget.NewTable(
			func() (int, int) { return len(rows), 2 },
			func() fyne.CanvasObject { return widget.NewLabel("") },
			func(id widget.TableCellID, o fyne.CanvasObject) {
				o.(*widget.Label).SetText(rows[id.Row][id.Col])
			},
		)
		for _, tab := range s.tabs.Items {
			if tab.Text == "Tables" {
				tab.Content = container.NewScroll(t)
				s.tabs.Refresh()
				s.tabs.Select(tab)
				return
			}
		}
		return
	}
	var doc tablesDoc
	if err := json.Unmarshal(out, &doc); err != nil {
		s.setStatus("error: bad acore json")
		return
	}
	rows := [][]string{{"Ref", "Tipo", "Valor"}}
	for _, p := range doc.Parts {
		rows = append(rows, []string{p.Ref, p.Kind, p.Value})
	}
	t := widget.NewTable(
		func() (int, int) { return len(rows), 3 },
		func() fyne.CanvasObject { return widget.NewLabel("") },
		func(id widget.TableCellID, o fyne.CanvasObject) {
			o.(*widget.Label).SetText(rows[id.Row][id.Col])
		},
	)
	for _, tab := range s.tabs.Items {
		if tab.Text == "Tables" {
			tab.Content = container.NewScroll(t)
			s.tabs.Refresh()
			s.tabs.Select(tab)
			return
		}
	}
}

func homeContent(s *state) fyne.CanvasObject {
	title := widget.NewLabelWithStyle("ATENEA — clon nativo interoperable",
		fyne.TextAlignLeading, fyne.TextStyle{Bold: true})
	pathEntry := widget.NewEntry()
	pathEntry.SetPlaceHolder("/ruta/al/archivo.pdsprj")
	open := widget.NewButton("Abrir", func() {
		if pathEntry.Text != "" {
			s.openFile(pathEntry.Text)
		}
	})
	cards := container.NewVBox(
		title,
		widget.NewLabel("Abrí un .pdsprj, .DSN, .MDF, .IDX para empezar."),
		pathEntry,
		open,
		widget.NewLabel(".pdsprj  .MDF  .SDF  .DSN  .LYT  .IDX"),
	)
	return container.NewScroll(cards)
}

func main() {
	a := app.NewWithID("atenea.go")
	w := a.NewWindow("Atenea")
	w.Resize(fyne.NewSize(1280, 800))
	st := &state{w: w, status: widget.NewLabel("listo")}
	st.tabs = container.NewAppTabs(
		container.NewTabItem("Tables", widget.NewLabel("sin archivo")),
		container.NewTabItem("Graph", widget.NewLabel("vista topológica (próximo slice)")),
		container.NewTabItem("Esquema", widget.NewLabel("hoja ISIS (próximo slice)")),
		container.NewTabItem("Simulate", widget.NewLabel("simulación batch (próximo slice)")),
	)
	openItem := fyne.NewMenuItem("Abrir…", func() {
		st.tabs.SelectIndex(0)
	})
	fileMenu := fyne.NewMenu("File", openItem, fyne.NewMenuItem("Salir", func() { a.Quit() }))
	helpMenu := fyne.NewMenu("Help", fyne.NewMenuItem("Acerca de", func() {
		st.setStatus("Atenea GPL-3.0-or-later · RE interoperable")
	}))
	w.SetMainMenu(fyne.NewMainMenu(fileMenu, helpMenu))
	home := homeContent(st)
	split := container.NewHSplit(home, st.tabs)
	split.Offset = 0.28
	w.SetContent(container.NewBorder(nil, st.status, nil, nil, split))
	if len(os.Args) > 1 {
		st.openFile(os.Args[1])
	}
	w.ShowAndRun()
}

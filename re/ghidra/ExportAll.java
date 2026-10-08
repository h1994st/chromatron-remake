// Exports decompiled C, the function list and strings (with referencing functions) for one program.
//@category Chromatron
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.data.StringDataInstance;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import java.io.File;
import java.io.PrintWriter;

public class ExportAll extends GhidraScript {
    @Override
    protected void run() throws Exception {
        File out = new File(getScriptArgs()[0]);
        out.mkdirs();
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        try (PrintWriter c = new PrintWriter(new File(out, "decompiled.c"), "UTF-8");
             PrintWriter fns = new PrintWriter(new File(out, "functions.tsv"), "UTF-8")) {
            fns.println("address\tname\tsize");
            for (Function fn : currentProgram.getFunctionManager().getFunctions(true)) {
                monitor.checkCancelled();
                fns.printf("%s\t%s\t%d%n", fn.getEntryPoint(), fn.getName(), fn.getBody().getNumAddresses());
                DecompileResults res = decompiler.decompileFunction(fn, 60, monitor);
                c.printf("// %s @ %s%n", fn.getName(), fn.getEntryPoint());
                c.println(res.decompileCompleted()
                    ? res.getDecompiledFunction().getC()
                    : "// decompile failed: " + res.getErrorMessage());
            }
        } finally {
            decompiler.dispose();
        }
        try (PrintWriter s = new PrintWriter(new File(out, "strings.tsv"), "UTF-8")) {
            s.println("address\txrefs\ttext");
            for (Data d : currentProgram.getListing().getDefinedData(true)) {
                if (!StringDataInstance.isString(d)) continue;
                String text = StringDataInstance.getStringDataInstance(d).getStringValue();
                if (text == null) continue;
                StringBuilder refs = new StringBuilder();
                for (Reference r : getReferencesTo(d.getAddress())) {
                    Function from = getFunctionContaining(r.getFromAddress());
                    if (refs.length() > 0) refs.append(',');
                    refs.append(from != null ? from.getName() : r.getFromAddress().toString());
                }
                s.printf("%s\t%s\t%s%n", d.getAddress(), refs,
                    text.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r"));
            }
        }
    }
}

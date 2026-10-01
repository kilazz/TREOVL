#!/usr/bin/env python3
import json
import os
import struct
import threading
import tkinter as tk
import zlib
from tkinter import filedialog, messagebox, ttk

HEADER_SIZE = 176
FOOTER_SIZE = 16
MAGIC_FOOTER_1 = 0xDEADBEEF
MAGIC_FOOTER_2 = 0xFEEDDEAF

# =====================================================================
# ЯДРО: UNPACKER & PACKER
# =====================================================================


class PRPUnpacker:
    def __init__(self, filepath, log_callback=None):
        self.filepath = filepath
        self.log = log_callback or print
        self.chunks_dir = ""
        self.chunk_counter = 0

    def unpack(self, output_dir):
        os.makedirs(output_dir, exist_ok=True)
        self.chunks_dir = os.path.join(output_dir, "chunks")
        os.makedirs(self.chunks_dir, exist_ok=True)

        with open(self.filepath, "rb") as f:
            content = f.read()

        if len(content) < HEADER_SIZE:
            raise ValueError(f"Файл поврежден или слишком мал: {len(content)} байт")

        header_bytes = content[:HEADER_SIZE]
        magic, maj_ver, min_ver, file_id, data_size = struct.unpack(
            "<4sHHII", header_bytes[:16]
        )
        pack_name = (
            header_bytes[16:176].split(b"\x00")[0].decode("latin1", errors="ignore")
        )

        header_info = {
            "magic": magic.decode("latin1", errors="ignore"),
            "major_version": maj_ver,
            "minor_version": min_ver,
            "file_id": file_id,
            "data_size": data_size,
            "pack_name": pack_name,
        }

        has_footer = False
        footer_hash2 = 0x7C809B8B
        payload_data = content[HEADER_SIZE:]

        if len(content) >= HEADER_SIZE + FOOTER_SIZE:
            potential_footer = content[-FOOTER_SIZE:]
            f_m1, f_m2, _f_crc, f_h2 = struct.unpack("<IIII", potential_footer)
            if f_m1 == MAGIC_FOOTER_1 and f_m2 == MAGIC_FOOTER_2:
                has_footer = True
                footer_hash2 = f_h2
                payload_data = content[HEADER_SIZE:-FOOTER_SIZE]

        self.log(
            f"[*] Заголовок: {header_info['magic']} v{maj_ver}.{min_ver} ('{pack_name}')"
        )
        self.log(
            f"[*] Футер: {'Обнаружен (DEADBEEF)' if has_footer else 'Отсутствует'}"
        )

        root_node = self._parse_node(
            payload_data, node_id=0, is_large=True, is_root=True
        )

        manifest = {
            "header": header_info,
            "has_footer": has_footer,
            "footer_hash2": footer_hash2,
            "root": root_node,
        }

        with open(os.path.join(output_dir, "project.json"), "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=4, ensure_ascii=False)

        self.log(f"[+] Извлечено чанков: {self.chunk_counter}")

    def _parse_node(self, data, node_id, is_large, is_root=False):
        if not data:
            return self._save_leaf(b"", node_id, is_large)

        pos = 0
        has_magic = False

        if not is_root:
            if data.startswith(b"\x01\x01\x00"):
                has_magic = True
                pos = 3
            elif not (data[0] & 0x80):
                return self._save_leaf(data, node_id, is_large)

        if len(data) <= pos:
            return self._save_leaf(data, node_id, is_large)

        control_byte = data[pos]
        has_large_entries = (control_byte & 0x80) != 0
        small_count = control_byte & 0x7F
        curr = pos + 1
        large_count = 0

        if has_large_entries:
            if curr + 4 > len(data):
                return self._save_leaf(data, node_id, is_large)
            large_count = struct.unpack("<I", data[curr : curr + 4])[0]
            curr += 4

        total_entries = small_count + large_count
        if total_entries == 0 or total_entries > 4096:
            return self._save_leaf(data, node_id, is_large)

        table_size = (small_count * 2) + (large_count * 8)
        data_start = curr + table_size

        if data_start > len(data):
            return self._save_leaf(data, node_id, is_large)

        entries = []
        try:
            for _ in range(small_count):
                b_id, b_off = struct.unpack("<BB", data[curr : curr + 2])
                entries.append(
                    {
                        "id": b_id,
                        "offset": b_off,
                        "is_large": False,
                        "order": len(entries),
                    }
                )
                curr += 2

            for _ in range(large_count):
                b_id, b_off = struct.unpack("<II", data[curr : curr + 8])
                entries.append(
                    {
                        "id": b_id,
                        "offset": b_off,
                        "is_large": True,
                        "order": len(entries),
                    }
                )
                curr += 8

            sorted_entries = sorted(entries, key=lambda x: x["offset"])

            if sorted_entries[0]["offset"] != 0:
                return self._save_leaf(data, node_id, is_large)

            for e in sorted_entries:
                if data_start + e["offset"] > len(data):
                    return self._save_leaf(data, node_id, is_large)

            for i, entry in enumerate(sorted_entries):
                start = data_start + entry["offset"]
                end = (
                    data_start + sorted_entries[i + 1]["offset"]
                    if i + 1 < len(sorted_entries)
                    else len(data)
                )
                if end < start:
                    return self._save_leaf(data, node_id, is_large)
                entry["raw_data"] = data[start:end]

            entries.sort(key=lambda x: x["order"])

            children = []
            for entry in entries:
                child = self._parse_node(
                    entry["raw_data"],
                    entry["id"],
                    entry["is_large"],
                    is_root=False,
                )
                children.append(child)

            return {
                "id": node_id,
                "is_large": is_large,
                "is_container": True,
                "has_magic": has_magic,
                "control_byte": control_byte,
                "children": children,
            }
        except (struct.error, ValueError, IndexError, UnicodeDecodeError):
            return self._save_leaf(data, node_id, is_large)

    def _save_leaf(self, data, node_id, is_large):
        self.chunk_counter += 1
        filename = f"chunk_{self.chunk_counter:04d}_id0x{node_id:X}.bin"
        filepath = os.path.join(self.chunks_dir, filename)

        with open(filepath, "wb") as f:
            f.write(data)

        return {
            "id": node_id,
            "is_large": is_large,
            "is_container": False,
            "file": f"chunks/{filename}",
            "size": len(data),
        }


class PRPPacker:
    def __init__(self, project_dir, compression_level=9, log_callback=None):
        self.project_dir = project_dir
        self.compression_level = compression_level
        self.log = log_callback or print

    def pack(self, output_filepath):
        manifest_path = os.path.join(self.project_dir, "project.json")
        if not os.path.exists(manifest_path):
            raise FileNotFoundError(f"В папке нет файла project.json:\n{manifest_path}")

        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)

        header_info = manifest["header"]
        root_node = manifest["root"]

        self.log(f"[*] Сборка архива... (Уровень сжатия: {self.compression_level})")
        payload_bytes = self._build_node(root_node, is_root=True)

        # Заголовок (176 байт)
        header_bytes = bytearray(HEADER_SIZE)
        magic_bytes = header_info["magic"].encode("latin1").ljust(4, b"\x00")
        header_bytes[0:4] = magic_bytes
        struct.pack_into(
            "<HH",
            header_bytes,
            4,
            header_info["major_version"],
            header_info["minor_version"],
        )
        struct.pack_into("<I", header_bytes, 8, header_info["file_id"])

        # ТОЧНЫЙ РАЗМЕР: Чистый размер собранного тела (payload)
        struct.pack_into("<I", header_bytes, 12, len(payload_bytes))

        name_bytes = header_info["pack_name"].encode("latin1")[:160]
        header_bytes[16 : 16 + len(name_bytes)] = name_bytes

        final_binary = bytearray(header_bytes) + payload_bytes

        # Генерация футера 0xDEADBEEF
        if manifest.get("has_footer", True):
            calculated_crc = (~zlib.crc32(final_binary)) & 0xFFFFFFFF
            footer_hash2 = manifest.get("footer_hash2", 0x7C809B8B)
            footer = struct.pack(
                "<IIII",
                MAGIC_FOOTER_1,
                MAGIC_FOOTER_2,
                calculated_crc,
                footer_hash2,
            )
            final_binary.extend(footer)
            self.log(f"[+] Рассчитан CRC32: 0x{calculated_crc:08X}")

        with open(output_filepath, "wb") as f:
            f.write(final_binary)

        self.log(
            f"[+] Архив успешно сохранен: {output_filepath} ({len(final_binary)} байт)"
        )

    def _build_node(self, node, is_root=False):
        if not node["is_container"]:
            chunk_path = os.path.join(self.project_dir, node["file"])
            with open(chunk_path, "rb") as f:
                data = f.read()

            if len(data) > 2 and data[:2] in [
                b"\x78\x9c",
                b"\x78\xda",
                b"\x78\x01",
                b"\x78\x5e",
            ]:
                try:
                    decompressed = zlib.decompress(data)
                    if self.compression_level == 0:
                        return data
                    return zlib.compress(decompressed, level=self.compression_level)
                except zlib.error:
                    return data
            return data

        children = node["children"]
        child_buffers = [self._build_node(c) for c in children]

        small_entries = []
        large_entries = []
        current_offset = 0

        for child, c_bin in zip(children, child_buffers):
            c_id = child["id"]
            is_large = child.get("is_large", False)

            if not is_large and c_id <= 255 and current_offset <= 255:
                small_entries.append((c_id, current_offset))
            else:
                large_entries.append((c_id, current_offset))

            current_offset += len(c_bin)

        table = bytearray()
        if node.get("has_magic", False):
            table.extend(b"\x01\x01\x00")

        has_large = len(large_entries) > 0
        control_byte = len(small_entries) & 0x7F
        if has_large:
            control_byte |= 0x80

        table.append(control_byte)
        if has_large:
            table.extend(struct.pack("<I", len(large_entries)))

        for c_id, off in small_entries:
            table.extend(struct.pack("<BB", c_id, off))

        for c_id, off in large_entries:
            table.extend(struct.pack("<II", c_id, off))

        return bytes(table) + b"".join(child_buffers)


# =====================================================================
# ГРАФИЧЕСКИЙ ИНТЕРФЕЙС (TKINTER + TTK)
# =====================================================================


class PRPToolGUI(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Overlord PRP Tool (Unpacker & Packer)")
        self.geometry("750x620")
        self.minsize(650, 500)

        self.style = ttk.Style(self)
        self.style.theme_use("clam")

        self._build_ui()

    def _build_ui(self):
        self.notebook = ttk.Notebook(self)
        self.notebook.pack(fill="both", expand=True, padx=10, pady=5)

        self.tab_unpack = ttk.Frame(self.notebook)
        self.tab_pack = ttk.Frame(self.notebook)
        self.tab_test = ttk.Frame(self.notebook)
        self.tab_fixer = ttk.Frame(self.notebook)

        self.notebook.add(self.tab_unpack, text=" 📂 Распаковка (Unpack) ")
        self.notebook.add(self.tab_pack, text=" 📦 Сборка (Pack) ")
        self.notebook.add(self.tab_test, text=" 🧪 Тест (Round-trip) ")
        self.notebook.add(self.tab_fixer, text=" 🩹 Быстрый CRC-фиксер ")

        self._init_unpack_tab()
        self._init_pack_tab()
        self._init_test_tab()
        self._init_fixer_tab()

        log_frame = ttk.LabelFrame(self, text="Лог выполнения")
        log_frame.pack(fill="both", expand=True, padx=10, pady=(0, 10))

        self.log_text = tk.Text(
            log_frame,
            height=10,
            bg="#1e1e1e",
            fg="#00ff66",
            insertbackground="white",
            font=("Consolas", 10),
        )
        self.log_text.pack(side="left", fill="both", expand=True, padx=5, pady=5)

        scrollbar = ttk.Scrollbar(
            log_frame, orient="vertical", command=self.log_text.yview
        )
        scrollbar.pack(side="right", fill="y", pady=5)
        self.log_text.config(yscrollcommand=scrollbar.set)

    def log(self, message):
        def _append():
            self.log_text.insert(tk.END, message + "\n")
            self.log_text.see(tk.END)

        self.after(0, _append)

    def _init_unpack_tab(self):
        frame = ttk.Frame(self.tab_unpack, padding=15)
        frame.pack(fill="both", expand=True)

        ttk.Label(
            frame,
            text="Выберите исходный архив (.prp, .rpk, .pvp, .psp, .omp):",
        ).pack(anchor="w")

        f_in = ttk.Frame(frame)
        f_in.pack(fill="x", pady=(2, 10))
        self.unpack_in_var = tk.StringVar()
        ttk.Entry(f_in, textvariable=self.unpack_in_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_in, text="Обзор...", command=self._browse_unpack_file).pack(
            side="right"
        )

        ttk.Label(frame, text="Папка назначения для проекта:").pack(anchor="w")
        f_out = ttk.Frame(frame)
        f_out.pack(fill="x", pady=(2, 15))
        self.unpack_out_var = tk.StringVar()
        ttk.Entry(f_out, textvariable=self.unpack_out_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_out, text="Папка...", command=self._browse_unpack_dir).pack(
            side="right"
        )

        btn = ttk.Button(frame, text="🚀 Распаковать архив", command=self._run_unpack)
        btn.pack(pady=5, ipadx=10, ipady=5)

    def _browse_unpack_file(self):
        f = filedialog.askopenfilename(
            filetypes=[
                (
                    "Overlord Packages",
                    "*.prp *.rpk *.pvp *.psp *.omp *.spk *.bin",
                ),
                ("All Files", "*.*"),
            ]
        )
        if f:
            self.unpack_in_var.set(f)
            base_dir = os.path.splitext(f)[0] + "_project"
            self.unpack_out_var.set(base_dir)

    def _browse_unpack_dir(self):
        d = filedialog.askdirectory()
        if d:
            self.unpack_out_var.set(d)

    def _run_unpack(self):
        src = self.unpack_in_var.get().strip()
        dst = self.unpack_out_var.get().strip()
        if not src or not os.path.exists(src):
            messagebox.showerror("Ошибка", "Укажите существующий файл архива!")
            return
        if not dst:
            messagebox.showerror("Ошибка", "Укажите папку для распаковки!")
            return

        def _worker():
            self.log(f"--- НАЧАЛО РАСПАКОВКИ: {os.path.basename(src)} ---")
            try:
                unpacker = PRPUnpacker(src, log_callback=self.log)
                unpacker.unpack(dst)
                self.log("[✓] Готово! Проект готов к редактированию.\n")
            except Exception as e:  # noqa: BLE001
                self.log(f"[!] Ошибка распаковки: {e}\n")

        threading.Thread(target=_worker, daemon=True).start()

    def _init_pack_tab(self):
        frame = ttk.Frame(self.tab_pack, padding=15)
        frame.pack(fill="both", expand=True)

        ttk.Label(frame, text="Папка проекта (содержащая project.json):").pack(
            anchor="w"
        )
        f_in = ttk.Frame(frame)
        f_in.pack(fill="x", pady=(2, 10))
        self.pack_in_var = tk.StringVar()
        ttk.Entry(f_in, textvariable=self.pack_in_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_in, text="Выбрать папку...", command=self._browse_pack_dir).pack(
            side="right"
        )

        ttk.Label(frame, text="Сохранить готовый архив как:").pack(anchor="w")
        f_out = ttk.Frame(frame)
        f_out.pack(fill="x", pady=(2, 15))
        self.pack_out_var = tk.StringVar()
        ttk.Entry(f_out, textvariable=self.pack_out_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_out, text="Куда сохранить...", command=self._browse_pack_out).pack(
            side="right"
        )

        comp_frame = ttk.LabelFrame(frame, text="Параметры сжатия ZLIB")
        comp_frame.pack(fill="x", pady=(5, 15), padx=5, ipadx=5, ipady=5)

        lbl_box = ttk.Frame(comp_frame)
        lbl_box.pack(fill="x", padx=5)
        ttk.Label(lbl_box, text="Уровень сжатия:").pack(side="left")
        self.comp_val_lbl = ttk.Label(
            lbl_box,
            text="9 (Максимальное)",
            font=("Arial", 9, "bold"),
            foreground="#0066cc",
        )
        self.comp_val_lbl.pack(side="left", padx=5)

        self.comp_var = tk.IntVar(value=9)
        self.comp_slider = ttk.Scale(
            comp_frame,
            from_=0,
            to=9,
            orient="horizontal",
            variable=self.comp_var,
            command=self._on_slider_change,
        )
        self.comp_slider.pack(fill="x", padx=10, pady=5)

        ttk.Label(
            comp_frame,
            text="0 = Без сжатия | 1 = Быстрое | 6 = Баланс | 9 = Максимальное",
            font=("Arial", 8),
            foreground="#666666",
        ).pack()

        btn = ttk.Button(frame, text="📦 Собрать PRP архив", command=self._run_pack)
        btn.pack(pady=10, ipadx=10, ipady=5)

    def _on_slider_change(self, val):
        v = round(float(val))
        desc = {
            0: "0 (Без сжатия / Store)",
            1: "1 (Сверхбыстрое)",
            6: "6 (Стандартное)",
            9: "9 (Максимальное)",
        }.get(v, f"{v}")
        self.comp_val_lbl.config(text=desc)

    def _browse_pack_dir(self):
        d = filedialog.askdirectory()
        if d:
            self.pack_in_var.set(d)
            self.pack_out_var.set(os.path.join(d, "rebuilt.prp"))

    def _browse_pack_out(self):
        f = filedialog.asksaveasfilename(
            defaultextension=".prp",
            filetypes=[("PRP Package", "*.prp"), ("RPK Package", "*.rpk")],
        )
        if f:
            self.pack_out_var.set(f)

    def _run_pack(self):
        src = self.pack_in_var.get().strip()
        dst = self.pack_out_var.get().strip()
        comp = self.comp_var.get()

        if not src or not os.path.exists(src):
            messagebox.showerror("Ошибка", "Укажите существующую папку проекта!")
            return
        if not dst:
            messagebox.showerror("Ошибка", "Укажите путь для сохранения архива!")
            return

        def _worker():
            self.log(f"--- НАЧАЛО СБОРКИ: {os.path.basename(dst)} ---")
            try:
                packer = PRPPacker(src, compression_level=comp, log_callback=self.log)
                packer.pack(dst)
                self.log("[✓] Успешно! Файл готов для использования в игре.\n")
            except Exception as e:  # noqa: BLE001
                self.log(f"[!] Ошибка упаковки: {e}\n")

        threading.Thread(target=_worker, daemon=True).start()

    def _init_test_tab(self):
        frame = ttk.Frame(self.tab_test, padding=15)
        frame.pack(fill="both", expand=True)

        ttk.Label(
            frame,
            text="Тест побайтовой идентичности (Unpack -> Pack -> Diff):",
            font=("Arial", 10, "bold"),
        ).pack(anchor="w")
        ttk.Label(
            frame,
            text="Скрипт распакует оригинал во временную папку, соберет заново\nи побайтово сравнит результат.",
        ).pack(anchor="w", pady=(2, 10))

        f_in = ttk.Frame(frame)
        f_in.pack(fill="x", pady=5)
        self.test_in_var = tk.StringVar()
        ttk.Entry(f_in, textvariable=self.test_in_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_in, text="Выбрать...", command=self._browse_test_file).pack(
            side="right"
        )

        btn = ttk.Button(frame, text="🧪 Запустить тест", command=self._run_test)
        btn.pack(pady=15, ipadx=10, ipady=5)

    def _browse_test_file(self):
        f = filedialog.askopenfilename(
            filetypes=[
                ("Overlord Files", "*.prp *.rpk *.pvp *.psp *.omp"),
                ("All", "*.*"),
            ]
        )
        if f:
            self.test_in_var.set(f)

    def _run_test(self):
        src = self.test_in_var.get().strip()
        if not src or not os.path.exists(src):
            messagebox.showerror("Ошибка", "Укажите файл для теста!")
            return

        def _worker():
            self.log(f"--- КРУГОВОЙ ТЕСТ: {os.path.basename(src)} ---")
            temp_dir = "__temp_gui_test"
            temp_out = "__temp_gui_rebuilt.prp"
            try:
                self.log("[1/3] Распаковка...")
                unpacker = PRPUnpacker(src, log_callback=self.log)
                unpacker.unpack(temp_dir)

                self.log("[2/3] Сборка...")
                packer = PRPPacker(temp_dir, compression_level=0, log_callback=self.log)
                packer.pack(temp_out)

                self.log("[3/3] Сравнение байтов...")
                with open(src, "rb") as f1, open(temp_out, "rb") as f2:
                    b1 = f1.read()
                    b2 = f2.read()

                if b1 == b2:
                    self.log(
                        "🎉 [100% УСПЕХ!] Пересобранный файл ПОБАЙТОВО ИДЕНТИЧЕН оригиналу!\n"
                    )
                else:
                    self.log(
                        f"[!] Размеры: Оригинал={len(b1)} | Пересобранный={len(b2)}"
                    )
                    if len(b1) == len(b2):
                        diffs = sum(1 for x, y in zip(b1, b2) if x != y)
                        self.log(f"[!] Отличается в {diffs} байтах.\n")
                    else:
                        self.log("[!] Длины файлов не равны.\n")
            except Exception as e:  # noqa: BLE001
                self.log(f"[!] Ошибка теста: {e}\n")

        threading.Thread(target=_worker, daemon=True).start()

    def _init_fixer_tab(self):
        frame = ttk.Frame(self.tab_fixer, padding=15)
        frame.pack(fill="both", expand=True)

        ttk.Label(
            frame,
            text="Быстрое исправление CRC-футера (после Hex-редактора):",
            font=("Arial", 10, "bold"),
        ).pack(anchor="w")
        ttk.Label(
            frame,
            text="Если вы вручную изменили байты в .prp файле через 010 Editor/HxD,\nэтот инструмент пересчитает и запишет правильный CRC32 в футер.",
        ).pack(anchor="w", pady=(2, 10))

        f_in = ttk.Frame(frame)
        f_in.pack(fill="x", pady=5)
        self.fixer_in_var = tk.StringVar()
        ttk.Entry(f_in, textvariable=self.fixer_in_var).pack(
            side="left", fill="x", expand=True, padx=(0, 5)
        )
        ttk.Button(f_in, text="Выбрать файл...", command=self._browse_fixer_file).pack(
            side="right"
        )

        btn = ttk.Button(frame, text="🩹 Исправить CRC32", command=self._run_fixer)
        btn.pack(pady=15, ipadx=10, ipady=5)

    def _browse_fixer_file(self):
        f = filedialog.askopenfilename(
            filetypes=[
                ("Overlord Files", "*.prp *.rpk *.pvp *.psp *.omp"),
                ("All", "*.*"),
            ]
        )
        if f:
            self.fixer_in_var.set(f)

    def _run_fixer(self):
        filepath = self.fixer_in_var.get().strip()
        if not filepath or not os.path.exists(filepath):
            messagebox.showerror("Ошибка", "Укажите файл для исправления!")
            return

        try:
            with open(filepath, "rb") as f:
                data = f.read()

            body = data[:-16]
            new_crc = (~zlib.crc32(body)) & 0xFFFFFFFF
            old_hash2 = data[-4:]

            new_footer = (
                struct.pack("<II", MAGIC_FOOTER_1, MAGIC_FOOTER_2)
                + struct.pack("<I", new_crc)
                + old_hash2
            )

            with open(filepath, "wb") as f:
                f.write(body)
                f.write(new_footer)

            self.log(f"[✓] Файл '{os.path.basename(filepath)}' успешно пропатчен!")
            self.log(f"    - Новый CRC32 в футере: 0x{new_crc:08X}\n")
            messagebox.showinfo("Успех", f"CRC32 успешно обновлен:\n0x{new_crc:08X}")
        except Exception as e:  # noqa: BLE001
            self.log(f"[!] Ошибка: {e}\n")


if __name__ == "__main__":
    app = PRPToolGUI()
    app.mainloop()

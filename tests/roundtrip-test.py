# -*- coding: utf-8 -*-
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import Select
from selenium.webdriver.chrome.options import Options
import time
import os
import sys

if __name__ == "__main__":
    test_file = sys.argv[1]

    chrome_options = Options()
    chrome_options.add_argument("--headless")
    driver = webdriver.Chrome(options=chrome_options)
    driver.implicitly_wait(30)

    EXP_OUTPUTS = {
        "roundtrip1.sh": """Connected
1.1 Sleep for 1
1.2 Sleep for 1
1.3 Sleep for 1
1.4 Done
Disconnected""",
        "roundtrip2.sh": """Connected
2.1 Sleep for 2
2.2 Sleep for 1
2.3 Done
Disconnected""",
        "roundtrip3.sh": """Connected
3.1 Sleep for 1
3.2 Sleep for 2
3.3 Done
Disconnected""",
        "roundtrip4.sh": """Connected
4.1 Sleep for 3
4.2 Done
Disconnected""",
    }

    try:
        root_path = os.path.normpath(os.getcwd() + "/tests/stage/scripts")
        driver.get("http://localhost:8080/")
        Select(driver.find_element(By.ID, "command")
               ).select_by_visible_text("sh")
        driver.find_element(By.ID, "file_path").send_keys("a/a")
        driver.find_element(By.ID, "file").send_keys(
            f"{root_path}/{test_file}")
        driver.find_element(By.ID, "run").click()
        time.sleep(5)  # should be sufficient for 3s wait
        all_output = []
        for x in driver.find_elements(By.XPATH, '//div[@id="output"]/pre'):
            all_output.append(x.text)
        # skip first line with non-unique file id
        all_output = "\n".join(all_output[1:])

        assert (EXP_OUTPUTS[test_file] ==
                all_output), f"""Output mismatch for {test_file}, expected:\n{EXP_OUTPUTS[test_file]}\n\nGot:\n{all_output}"""
    finally:
        driver.quit()

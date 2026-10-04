// @ts-check
import { total } from "./ledger";

/** @type {number} */
const sum = total([3, 4]).toFixed(2);
console.log(sum);
